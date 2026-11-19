//! GPU-рендерер кадра `orbita-ui` через virgl.
//!
//! ```mermaid
//! flowchart LR
//!     F[Frame: instances, batches] --> V[вершины 4×13 vec4 на примитив]
//!     F --> S[Path → спрайт: запечь один раз в атлас]
//!     F --> W[статичный пакет → текстура обоев]
//!     V --> U[TRANSFER_TO_HOST_3D]
//!     U --> C[SUBMIT_3D: draw по пакетам]
//!     C -->|capture| B[копия сцены → 1/2 → 1/4 → 1/8]
//!     B --> C
//!     C --> P[RESOURCE_FLUSH → окно QEMU]
//! ```
//!
//! Все пиксели интерфейса считает фрагментный шейдер на видеокарте хоста;
//! процессор гостя только раскладывает примитивы и отправляет команды.

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use hashbrown::HashMap;

use orbita_ui::gfx::soft::rasterize_range;
use orbita_ui::gfx::soft::rasterize_sprite;
use orbita_ui::gfx::text::{ATLAS_SIZE, Atlas};
use orbita_ui::gfx::{Frame, Instance, Kind};

use crate::encoder::{Encoder, SHADER_FRAGMENT, SHADER_VERTEX, bind, fmt, obj};
use crate::shaders;

/// Описание ресурса для [`GpuDevice::create_resource`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceDesc {
    /// 0 — буфер, 2 — 2D-текстура.
    pub target: u32,
    /// Формат virgl.
    pub format: u32,
    /// Флаги `VIRGL_BIND_*`.
    pub bind: u32,
    /// Ширина (для буфера — байты).
    pub width: u32,
    /// Высота.
    pub height: u32,
}

/// Устройство, исполняющее команды (в ядре — `VirtioGpu`, в тестах — запись).
pub trait GpuDevice {
    /// Создаёт ресурс и привязывает к 3D-контексту. Возвращает его id.
    fn create_resource(&mut self, desc: ResourceDesc) -> Result<u32, String>;
    /// Подключает гостевую память как хранилище ресурса.
    fn attach_backing(&mut self, res: u32, addr: u64, len: u32) -> Result<(), String>;
    /// Ставит в очередь копирование области backing → GPU.
    fn upload(&mut self, res: u32, x: u32, y: u32, w: u32, h: u32, offset: u64, stride: u32) -> Result<(), String>;
    /// Ставит в очередь поток команд virgl.
    fn submit(&mut self, cmds: &[u32]) -> Result<(), String>;
    /// Назначает ресурс изображением экрана.
    fn set_scanout(&mut self, res: u32, w: u32, h: u32) -> Result<(), String>;
    /// Ставит в очередь показ ресурса на экране.
    fn present(&mut self, res: u32, w: u32, h: u32) -> Result<(), String>;
    /// Ждёт исполнения всего поставленного.
    fn finish(&mut self) -> Result<(), String>;
}

/// Байт на вершину: 13 × vec4 float.
pub const VERTEX_BYTES: usize = shaders::UI_ATTRS as usize * 16;
/// Максимум примитивов за кадр.
pub const MAX_INSTANCES: usize = 8192;
/// Сторона атласа спрайтов.
pub const SPRITE_ATLAS: u32 = 2048;

// Handles объектов virgl (произвольные, уникальные в контексте).
const H_BLEND: u32 = 1;
const H_BLEND_COPY: u32 = 2;
const H_RAST: u32 = 3;
const H_DSA: u32 = 4;
const H_VE_UI: u32 = 5;
const H_VE_BLIT: u32 = 6;
const H_VS_UI: u32 = 10;
const H_FS_UI: u32 = 11;
const H_VS_BLIT: u32 = 12;
const H_FS_DOWN: u32 = 13;
const H_SURF_SCENE: u32 = 20;
const H_SURF_PYR: u32 = 21; // + 0..2
const H_VIEW_ATLAS: u32 = 30;
const H_VIEW_SPRITES: u32 = 31;
const H_VIEW_WALL: u32 = 33;
const H_VIEW_CAP: u32 = 34;
const H_VIEW_PYR: u32 = 35; // + 0..2
const H_SAMP_LINEAR: u32 = 40;

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

/// Атлас спрайтов (запечённые векторные иконки и курсоры).
struct Sprites {
    pixels: Vec<u32>,
    shelves: Vec<Shelf>,
    next_y: u32,
    map: HashMap<u64, [u32; 4]>,
    dirty: Option<[u32; 4]>,
    resets: u32,
}

impl Sprites {
    fn new() -> Sprites {
        Sprites {
            pixels: vec![0u32; (SPRITE_ATLAS * SPRITE_ATLAS) as usize],
            shelves: Vec::new(),
            next_y: 0,
            map: HashMap::new(),
            dirty: None,
            resets: 0,
        }
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w, h) = (w + 1, h + 1);
        if w > SPRITE_ATLAS || h > SPRITE_ATLAS {
            return None;
        }
        for s in self.shelves.iter_mut() {
            if h <= s.h && s.x + w <= SPRITE_ATLAS && h * 4 >= s.h * 3 {
                let p = (s.x, s.y);
                s.x += w;
                return Some(p);
            }
        }
        if self.next_y + h > SPRITE_ATLAS {
            return None;
        }
        let y = self.next_y;
        self.next_y += h;
        self.shelves.push(Shelf { y, h, x: w });
        Some((0, y))
    }

    fn reset(&mut self) {
        self.shelves.clear();
        self.next_y = 0;
        self.map.clear();
        self.resets += 1;
    }

    fn mark(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let r = [x, y, x + w, y + h];
        self.dirty = Some(match self.dirty {
            None => r,
            Some(d) => [d[0].min(r[0]), d[1].min(r[1]), d[2].max(r[2]), d[3].max(r[3])],
        });
    }
}

fn hash_f32s(h: &mut u64, vals: &[f32]) {
    for v in vals {
        *h ^= v.to_bits() as u64;
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
}

/// Статистика последнего кадра.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuStats {
    /// Нарисовано примитивов.
    pub instances: usize,
    /// Снимков стекла (пирамид блюра).
    pub captures: usize,
    /// Спрайтов запечено в этом кадре.
    pub baked: usize,
    /// Байт вершин отправлено.
    pub vertex_bytes: usize,
}

/// Рендерер интерфейса на GPU через virgl.
pub struct GpuRenderer {
    w: u32,
    h: u32,
    res_scene: u32,
    res_cap: u32,
    /// Уровни пирамиды размытия (1/2, 1/4, 1/8) — держим id для диагностики.
    #[allow(dead_code)]
    res_pyr: [u32; 3],
    res_wall: u32,
    res_atlas: u32,
    res_sprites: u32,
    res_vbuf: u32,
    res_ibuf: u32,
    res_blit: u32,
    vbuf: Vec<u8>,
    ibuf: Vec<u32>,
    blit: Vec<f32>,
    wall: Vec<u32>,
    wall_key: u32,
    sprites: Sprites,
    atlas_gen: u32,
    enc: Encoder,
    insts: Vec<Instance>,
    /// Статистика последнего кадра.
    pub stats: GpuStats,
}

fn map_err(e: String) -> String {
    e
}

impl GpuRenderer {
    /// Создаёт ресурсы и объекты состояния на GPU.
    ///
    /// Принимает: устройство, размер экрана и атлас глифов (его память
    /// становится хранилищем текстуры атласа — без копий).
    pub fn new(dev: &mut dyn GpuDevice, w: u32, h: u32, atlas: &Atlas) -> Result<GpuRenderer, String> {
        let tex = |format, bind, w, h| ResourceDesc { target: 2, format, bind, width: w, height: h };
        let rt = bind::RENDER_TARGET | bind::SAMPLER_VIEW;
        let res_scene = dev.create_resource(tex(fmt::B8G8R8A8_UNORM, rt | bind::SCANOUT, w, h))?;
        let res_cap = dev.create_resource(tex(fmt::B8G8R8A8_UNORM, rt, w, h))?;
        let mut res_pyr = [0u32; 3];
        for (i, r) in res_pyr.iter_mut().enumerate() {
            let k = 2u32 << i;
            *r = dev.create_resource(tex(fmt::B8G8R8A8_UNORM, rt, (w / k).max(1), (h / k).max(1)))?;
        }
        let res_wall = dev.create_resource(tex(fmt::B8G8R8A8_UNORM, bind::SAMPLER_VIEW, w, h))?;
        let res_atlas = dev.create_resource(tex(fmt::R8_UNORM, bind::SAMPLER_VIEW, ATLAS_SIZE, ATLAS_SIZE))?;
        let res_sprites = dev.create_resource(tex(fmt::B8G8R8A8_UNORM, bind::SAMPLER_VIEW, SPRITE_ATLAS, SPRITE_ATLAS))?;
        let vbytes = MAX_INSTANCES * 4 * VERTEX_BYTES;
        let res_vbuf = dev.create_resource(ResourceDesc { target: 0, format: fmt::R8_UNORM, bind: bind::VERTEX_BUFFER, width: vbytes as u32, height: 1 })?;
        let ibytes = MAX_INSTANCES * 6 * 4;
        let res_ibuf = dev.create_resource(ResourceDesc { target: 0, format: fmt::R8_UNORM, bind: bind::INDEX_BUFFER, width: ibytes as u32, height: 1 })?;
        let res_blit = dev.create_resource(ResourceDesc { target: 0, format: fmt::R8_UNORM, bind: bind::VERTEX_BUFFER, width: 96, height: 1 })?;

        let vbuf = vec![0u8; vbytes];
        let mut ibuf = vec![0u32; MAX_INSTANCES * 6];
        for i in 0..MAX_INSTANCES {
            let b = (i * 4) as u32;
            ibuf[i * 6..i * 6 + 6].copy_from_slice(&[b, b + 1, b + 2, b + 2, b + 1, b + 3]);
        }
        // Полноэкранный квад: (ndc.x, ndc.y, u, v).
        let blit = vec![
            -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 0.0, 0.0, //
            -1.0, 1.0, 0.0, 0.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0f32,
        ];
        let wall = vec![0u32; (w * h) as usize];
        let sprites = Sprites::new();
        dev.attach_backing(res_vbuf, vbuf.as_ptr() as u64, vbuf.len() as u32)?;
        dev.attach_backing(res_ibuf, ibuf.as_ptr() as u64, (ibuf.len() * 4) as u32)?;
        dev.attach_backing(res_blit, blit.as_ptr() as u64, 96)?;
        dev.attach_backing(res_wall, wall.as_ptr() as u64, (wall.len() * 4) as u32)?;
        dev.attach_backing(res_atlas, atlas.pixels.as_ptr() as u64, atlas.pixels.len() as u32)?;
        dev.attach_backing(res_sprites, sprites.pixels.as_ptr() as u64, (sprites.pixels.len() * 4) as u32)?;
        dev.upload(res_ibuf, 0, 0, (ibuf.len() * 4) as u32, 1, 0, 0)?;
        dev.upload(res_blit, 0, 0, 96, 1, 0, 0)?;
        dev.set_scanout(res_scene, w, h)?;

        let mut e = Encoder::new();
        e.blend_premultiplied(H_BLEND);
        e.blend_opaque(H_BLEND_COPY);
        e.rasterizer(H_RAST);
        e.dsa_off(H_DSA);
        e.vertex_elements_vec4(H_VE_UI, shaders::UI_ATTRS);
        e.vertex_elements_vec4(H_VE_BLIT, 1);
        e.shader(H_VS_UI, SHADER_VERTEX, &shaders::ui_vertex());
        e.shader(H_FS_UI, SHADER_FRAGMENT, &shaders::ui_fragment());
        e.shader(H_VS_BLIT, SHADER_VERTEX, &shaders::blit_vertex());
        e.shader(H_FS_DOWN, SHADER_FRAGMENT, &shaders::downsample_fragment());
        e.surface(H_SURF_SCENE, res_scene, fmt::B8G8R8A8_UNORM);
        for i in 0..3 {
            e.surface(H_SURF_PYR + i as u32, res_pyr[i], fmt::B8G8R8A8_UNORM);
            e.sampler_view(H_VIEW_PYR + i as u32, res_pyr[i], fmt::B8G8R8A8_UNORM);
        }
        e.sampler_view(H_VIEW_ATLAS, res_atlas, fmt::R8_UNORM);
        e.sampler_view(H_VIEW_SPRITES, res_sprites, fmt::B8G8R8A8_UNORM);
        e.sampler_view(H_VIEW_WALL, res_wall, fmt::B8G8R8A8_UNORM);
        e.sampler_view(H_VIEW_CAP, res_cap, fmt::B8G8R8A8_UNORM);
        e.sampler_state(H_SAMP_LINEAR, true);
        e.bind_object(obj::RASTERIZER, H_RAST);
        e.bind_object(obj::DSA, H_DSA);
        e.framebuffer(H_SURF_SCENE);
        e.viewport(w as f32, h as f32);
        e.clear_color([0.12, 0.12, 0.12, 1.0]);
        dev.submit(&e.buf)?;
        dev.finish().map_err(map_err)?;

        Ok(GpuRenderer {
            w,
            h,
            res_scene,
            res_cap,
            res_pyr,
            res_wall,
            res_atlas,
            res_sprites,
            res_vbuf,
            res_ibuf,
            res_blit,
            vbuf,
            ibuf,
            blit,
            wall,
            wall_key: 0,
            sprites,
            atlas_gen: 0,
            enc: Encoder::new(),
            insts: Vec::new(),
            stats: GpuStats::default(),
        })
    }

    /// Размер экрана.
    pub fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }

    /// Состояние для отрисовки примитивов интерфейса в сцену.
    fn ui_state(&mut self) {
        let (w, h) = (self.w as f32, self.h as f32);
        let e = &mut self.enc;
        e.framebuffer(H_SURF_SCENE);
        e.viewport(w, h);
        e.bind_object(obj::BLEND, H_BLEND);
        e.bind_object(obj::VERTEX_ELEMENTS, H_VE_UI);
        e.bind_shader(H_VS_UI, SHADER_VERTEX);
        e.bind_shader(H_FS_UI, SHADER_FRAGMENT);
        e.vertex_buffer(VERTEX_BYTES as u32, 0, self.res_vbuf);
        e.index_buffer(self.res_ibuf);
        e.set_sampler_views(SHADER_FRAGMENT, 0, &[H_VIEW_ATLAS, H_VIEW_SPRITES, H_VIEW_PYR + 2, H_VIEW_WALL]);
        e.bind_sampler_states(SHADER_FRAGMENT, 0, &[H_SAMP_LINEAR; 4]);
        e.constants(SHADER_VERTEX, &[2.0 / w, -2.0 / h, -1.0, 1.0]);
        e.constants(SHADER_FRAGMENT, &[1.0 / w, 1.0 / h, 8.0 / w, 8.0 / h]);
    }

    /// Снимок сцены и пирамида размытия 1/2 → 1/4 → 1/8.
    fn capture(&mut self) {
        let (w, h) = (self.w, self.h);
        let e = &mut self.enc;
        e.copy_region(self.res_cap, 0, 0, self.res_scene, 0, 0, w, h);
        e.bind_object(obj::BLEND, H_BLEND_COPY);
        e.bind_object(obj::VERTEX_ELEMENTS, H_VE_BLIT);
        e.bind_shader(H_VS_BLIT, SHADER_VERTEX);
        e.bind_shader(H_FS_DOWN, SHADER_FRAGMENT);
        e.vertex_buffer(16, 0, self.res_blit);
        e.bind_sampler_states(SHADER_FRAGMENT, 0, &[H_SAMP_LINEAR]);
        let mut src_view = H_VIEW_CAP;
        let (mut sw, mut sh) = (w, h);
        for i in 0..3u32 {
            let k = 2u32 << i;
            let (dw, dh) = ((w / k).max(1), (h / k).max(1));
            e.framebuffer(H_SURF_PYR + i);
            e.viewport(dw as f32, dh as f32);
            e.set_sampler_views(SHADER_FRAGMENT, 0, &[src_view]);
            e.constants(SHADER_FRAGMENT, &[1.0 / sw as f32, 1.0 / sh as f32, 0.0, 0.0]);
            e.draw(0, 6, false);
            src_view = H_VIEW_PYR + i;
            sw = dw;
            sh = dh;
        }
        self.stats.captures += 1;
    }

    /// Ключ кэша спрайта: геометрия и цвет пути без абсолютного положения
    /// и без общей прозрачности.
    fn sprite_key(inst: &Instance, segs: &[[f32; 4]], dx: f32, dy: f32, alpha_k: f32) -> u64 {
        let mut i = *inst;
        i.rect[0] -= dx;
        i.rect[1] -= dy;
        i.uv[0] -= dx;
        i.uv[1] -= dy;
        i.grad0[1] -= dx;
        i.grad0[2] -= dy;
        if i.grad0[0] == 1.0 {
            i.grad1[0] -= dx;
            i.grad1[1] -= dy;
        }
        i.clip = [0.0; 4];
        i.extra[0] = 0.0;
        i.seg = [0.0; 4];
        for c in [&mut i.color0, &mut i.color1, &mut i.color2] {
            c[3] *= alpha_k;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for f in [&i.rect, &i.radii, &i.color0, &i.color1, &i.color2, &i.params, &i.grad0, &i.grad1, &i.extra, &i.uv] {
            hash_f32s(&mut h, f);
        }
        let s = inst.seg[0] as usize;
        let n = inst.seg[1] as usize;
        for seg in segs.iter().skip(s).take(n) {
            hash_f32s(&mut h, seg);
        }
        h
    }

    /// Превращает путь в спрайт (запекая при первом появлении).
    fn path_to_sprite(&mut self, inst: &Instance, frame: &Frame, atlas: &Atlas) -> Option<Instance> {
        // Квантуем дробную позицию до 1/4 пикселя — меньше вариантов в кэше.
        let fx = inst.rect[0] - libm::floorf(inst.rect[0]);
        let fy = inst.rect[1] - libm::floorf(inst.rect[1]);
        let sx = libm::roundf(fx * 4.0) / 4.0 - fx;
        let sy = libm::roundf(fy * 4.0) / 4.0 - fy;
        let mut p = *inst;
        for (x, y) in [(0usize, 1usize)] {
            p.rect[x] += sx;
            p.rect[y] += sy;
            p.uv[x] += sx;
            p.uv[y] += sy;
        }
        p.grad0[1] += sx;
        p.grad0[2] += sy;
        if p.grad0[0] == 1.0 {
            p.grad1[0] += sx;
            p.grad1[1] += sy;
        }
        // Общая прозрачность выносится в множитель спрайта.
        let amax = p.color0[3].max(p.color1[3]).max(p.color2[3]);
        if amax <= 0.002 {
            return None;
        }
        let k = 1.0 / amax;
        for c in [&mut p.color0, &mut p.color1, &mut p.color2] {
            c[3] = (c[3] * k).min(1.0);
        }
        let x0 = libm::floorf(p.rect[0]) as i32 - 1;
        let y0 = libm::floorf(p.rect[1]) as i32 - 1;
        let w = (libm::ceilf(p.rect[0] + p.rect[2]) as i32 + 1 - x0).max(1) as u32;
        let h = (libm::ceilf(p.rect[1] + p.rect[3]) as i32 + 1 - y0).max(1) as u32;
        let key = Self::sprite_key(&p, &frame.segments, x0 as f32, y0 as f32, 1.0);
        let region = match self.sprites.map.get(&key) {
            Some(r) => *r,
            None => {
                let pos = match self.sprites.alloc(w, h) {
                    Some(pos) => pos,
                    None => {
                        self.sprites.reset();
                        self.sprites.mark(0, 0, SPRITE_ATLAS, SPRITE_ATLAS);
                        self.sprites.alloc(w, h)?
                    }
                };
                let mut tmp = vec![0u32; (w * h) as usize];
                rasterize_sprite(&p, &frame.segments, atlas, x0, y0, w, h, &mut tmp);
                for row in 0..h {
                    let dst = ((pos.1 + row) * SPRITE_ATLAS + pos.0) as usize;
                    self.sprites.pixels[dst..dst + w as usize].copy_from_slice(&tmp[(row * w) as usize..((row + 1) * w) as usize]);
                }
                self.sprites.mark(pos.0, pos.1, w, h);
                self.stats.baked += 1;
                let r = [pos.0, pos.1, w, h];
                self.sprites.map.insert(key, r);
                r
            }
        };
        let inv = 1.0 / SPRITE_ATLAS as f32;
        let mut s = Instance::default();
        s.rect = [x0 as f32 - sx, y0 as f32 - sy, w as f32, h as f32];
        s.uv = [region[0] as f32 * inv, region[1] as f32 * inv, (region[0] + region[2]) as f32 * inv, (region[1] + region[3]) as f32 * inv];
        s.color0 = [1.0, 1.0, 1.0, amax];
        s.params[0] = 3.0;
        s.clip = inst.clip;
        s.extra[0] = inst.extra[0];
        s.extra[1] = 1.0;
        Some(s)
    }

    /// Рисует кадр на GPU и показывает его на экране.
    ///
    /// Принимает: устройство, кадр и атлас глифов, на который ссылается кадр.
    pub fn render(&mut self, dev: &mut dyn GpuDevice, frame: &Frame, atlas: &mut Atlas) -> Result<(), String> {
        self.stats = GpuStats::default();
        let full = [0.0, 0.0, self.w as f32, self.h as f32];

        // 1. Статичный пакет (обои) → текстура, только при смене ключа.
        let mut skip: Option<(usize, usize)> = None;
        for b in &frame.batches {
            if b.cache_key != 0 {
                skip = Some((b.start as usize, b.end as usize));
                if b.cache_key != self.wall_key {
                    rasterize_range(frame, atlas, b.start as usize, b.end as usize, &mut self.wall, self.w, self.h);
                    dev.upload(self.res_wall, 0, 0, self.w, self.h, 0, self.w * 4)?;
                    self.wall_key = b.cache_key;
                }
                break;
            }
        }

        // 2. Атлас глифов: только грязная область.
        let gen_changed = atlas.generation != self.atlas_gen;
        let dirty = atlas.take_dirty();
        if gen_changed {
            self.atlas_gen = atlas.generation;
            dev.upload(self.res_atlas, 0, 0, ATLAS_SIZE, ATLAS_SIZE, 0, ATLAS_SIZE)?;
        } else if let Some(d) = dirty {
            dev.upload(self.res_atlas, d[0], d[1], d[2] - d[0], d[3] - d[1], (d[1] * ATLAS_SIZE + d[0]) as u64, ATLAS_SIZE)?;
        }

        // 3. Список примитивов для GPU (пути → спрайты, обои → картинка).
        let mut insts = core::mem::take(&mut self.insts);
        insts.clear();
        let mut ranges: Vec<(usize, usize, bool)> = Vec::with_capacity(frame.batches.len() + 1);
        for b in &frame.batches {
            let start = insts.len();
            if b.cache_key != 0 {
                let mut img = Instance::default();
                img.rect = full;
                img.uv = [0.0, 0.0, 1.0, 1.0];
                img.color0 = [1.0; 4];
                img.params[0] = 6.0;
                img.clip = [0.0, 0.0, self.w as f32, self.h as f32];
                img.extra[1] = 1.0;
                insts.push(img);
            } else {
                for inst in &frame.instances[b.start as usize..b.end as usize] {
                    if insts.len() >= MAX_INSTANCES {
                        break;
                    }
                    if inst.params[0] as u32 == Kind::Path as u32 {
                        if let Some(s) = self.path_to_sprite(inst, frame, atlas) {
                            insts.push(s);
                        }
                    } else {
                        insts.push(*inst);
                    }
                }
            }
            ranges.push((start, insts.len(), b.capture != 0));
        }
        let _ = skip;
        if let Some(d) = self.sprites.dirty.take() {
            dev.upload(self.res_sprites, d[0], d[1], d[2] - d[0], d[3] - d[1], ((d[1] * SPRITE_ATLAS + d[0]) * 4) as u64, SPRITE_ATLAS * 4)?;
        }

        // 4. Вершины: 4 угла на примитив (квад расширен на 1px под сглаживание).
        let n = insts.len();
        let mut off = 0usize;
        for inst in &insts {
            let grow = 1.0;
            let x0 = inst.rect[0] - grow;
            let y0 = inst.rect[1] - grow;
            let x1 = inst.rect[0] + inst.rect[2] + grow;
            let y1 = inst.rect[1] + inst.rect[3] + grow;
            let fields: [&[f32; 4]; 12] = [
                &inst.rect, &inst.radii, &inst.color0, &inst.color1, &inst.color2, &inst.border, &inst.params,
                &inst.grad0, &inst.grad1, &inst.clip, &inst.extra, &inst.uv,
            ];
            for (cx, cy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                let v = &mut self.vbuf[off..off + VERTEX_BYTES];
                let mut o = 0;
                for f in [cx, cy, 0.0, 1.0] {
                    v[o..o + 4].copy_from_slice(&f.to_le_bytes());
                    o += 4;
                }
                for fld in fields {
                    for f in fld {
                        v[o..o + 4].copy_from_slice(&f.to_le_bytes());
                        o += 4;
                    }
                }
                off += VERTEX_BYTES;
            }
        }
        if off > 0 {
            dev.upload(self.res_vbuf, 0, 0, off as u32, 1, 0, 0)?;
        }
        self.stats.instances = n;
        self.stats.vertex_bytes = off;

        // 5. Команды: очистка, пакеты (со снимками стекла), показ.
        self.enc.clear();
        self.ui_state();
        self.enc.clear_color(frame.clear);
        for (start, end, capture) in ranges {
            if capture {
                self.capture();
                self.ui_state();
            }
            if end > start {
                self.enc.draw((start * 6) as u32, ((end - start) * 6) as u32, true);
            }
        }
        dev.submit(&self.enc.buf)?;
        dev.present(self.res_scene, self.w, self.h)?;
        self.insts = insts;
        let _ = (&self.ibuf, &self.blit, self.res_cap);
        dev.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbita_ui::{Desktop, FrameInput};

    /// Устройство-заглушка: считает вызовы и проверяет поток команд.
    struct Recorder {
        next: u32,
        submits: usize,
        uploads: usize,
        words: usize,
        presents: usize,
    }

    impl GpuDevice for Recorder {
        fn create_resource(&mut self, _d: ResourceDesc) -> Result<u32, String> {
            self.next += 1;
            Ok(self.next)
        }
        fn attach_backing(&mut self, _r: u32, addr: u64, len: u32) -> Result<(), String> {
            assert!(addr != 0 && len > 0);
            Ok(())
        }
        fn upload(&mut self, _r: u32, _x: u32, _y: u32, w: u32, h: u32, _o: u64, _s: u32) -> Result<(), String> {
            assert!(w > 0 && h > 0);
            self.uploads += 1;
            Ok(())
        }
        fn submit(&mut self, cmds: &[u32]) -> Result<(), String> {
            // Заголовки должны точно покрывать поток.
            let mut i = 0;
            while i < cmds.len() {
                i += 1 + (cmds[i] >> 16) as usize;
            }
            assert_eq!(i, cmds.len());
            self.submits += 1;
            self.words += cmds.len();
            Ok(())
        }
        fn set_scanout(&mut self, _r: u32, _w: u32, _h: u32) -> Result<(), String> {
            Ok(())
        }
        fn present(&mut self, _r: u32, _w: u32, _h: u32) -> Result<(), String> {
            self.presents += 1;
            Ok(())
        }
        fn finish(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn renders_desktop_frame() {
        let mut dev = Recorder { next: 0, submits: 0, uploads: 0, words: 0, presents: 0 };
        let mut d = Desktop::new(orbita_ui::system::DemoSystem::boxed());
        d.open(orbita_ui::apps::AppKind::Terminal, None);
        let input = FrameInput { width: 640, height: 400, scale: 1.0, time: 1.0, ..Default::default() };
        let mut r = GpuRenderer::new(&mut dev, 640, 400, &d.ui.p.fonts.atlas).unwrap();
        for i in 0..3 {
            let mut inp = input.clone();
            inp.time = 1.0 + i as f64;
            d.frame(&inp);
            let frame = d.ui.p.frame().clone();
            r.render(&mut dev, &frame, &mut d.ui.p.fonts.atlas).unwrap();
        }
        assert!(r.stats.instances > 50);
        assert!(r.stats.captures >= 2, "стекло окна и панелей должно снимать фон");
        assert_eq!(dev.presents, 3);
        // Иконки запекаются в первом кадре и потом берутся из кэша.
        assert_eq!(r.stats.baked, 0, "в установившемся кадре нет новых спрайтов");
    }
}
