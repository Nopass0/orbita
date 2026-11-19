//! Программный растеризатор кадра — для ядра (UEFI GOP framebuffer) и
//! для эталонных тестов.
//!
//! Повторяет математику WGSL-шейдера эмулятора (`emu/orbita-emu/src/ui.wgsl`)
//! один в один: SDF скруглённых прямоугольников, градиенты в
//! предумноженной альфе, тени через erf-аппроксимацию, контуры иконок по
//! расстоянию до отрезков, стекло, шум.
//!
//! Чтобы держать частоту кадров без GPU, рендерер перерисовывает только
//! изменившиеся области (damage tracking): сравнивает инстансы кадра с
//! прошлым кадром и закрашивает объединение их габаритов. Статичный пакет
//! (обои) рисуется один раз в кэш, из него же строится размытая копия для
//! стекла.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use hashbrown::HashMap;

use super::text::{ATLAS_SIZE, Atlas};
use super::{Frame, Instance, Kind};
use crate::geom::Rect;
use crate::math;

/// Предумноженный цвет на время смешивания.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Px {
    /// Красный × альфа.
    pub r: f32,
    /// Зелёный × альфа.
    pub g: f32,
    /// Синий × альфа.
    pub b: f32,
    /// Альфа.
    pub a: f32,
}

impl Px {
    #[inline]
    fn premul(c: [f32; 4]) -> Px {
        Px { r: c[0] * c[3], g: c[1] * c[3], b: c[2] * c[3], a: c[3] }
    }

    #[inline]
    fn scale(self, k: f32) -> Px {
        Px { r: self.r * k, g: self.g * k, b: self.b * k, a: self.a * k }
    }

    #[inline]
    fn mix(self, o: Px, t: f32) -> Px {
        Px {
            r: math::lerp(self.r, o.r, t),
            g: math::lerp(self.g, o.g, t),
            b: math::lerp(self.b, o.b, t),
            a: math::lerp(self.a, o.a, t),
        }
    }

    #[inline]
    fn over(self, dst: Px) -> Px {
        let k = 1.0 - self.a;
        Px { r: self.r + dst.r * k, g: self.g + dst.g * k, b: self.b + dst.b * k, a: self.a + dst.a * k }
    }
}

#[inline]
fn unpack(v: u32) -> Px {
    Px {
        r: ((v >> 16) & 0xff) as f32 / 255.0,
        g: ((v >> 8) & 0xff) as f32 / 255.0,
        b: (v & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

#[inline]
fn pack(p: Px) -> u32 {
    let r = (math::clamp(p.r, 0.0, 1.0) * 255.0 + 0.5) as u32;
    let g = (math::clamp(p.g, 0.0, 1.0) * 255.0 + 0.5) as u32;
    let b = (math::clamp(p.b, 0.0, 1.0) * 255.0 + 0.5) as u32;
    0xff00_0000 | (r << 16) | (g << 8) | b
}

// ------------------------------------------------------------- SDF-функции

/// Знаковое расстояние до скруглённого прямоугольника.
///
/// Принимает: точку `(px, py)`, прямоугольник `[x, y, w, h]`, радиусы
/// `[tl, tr, br, bl]`. Возвращает: <0 внутри, >0 снаружи (px).
///
/// ```
/// use orbita_ui::gfx::soft::sd_round_rect;
/// let r = [0.0, 0.0, 100.0, 50.0];
/// assert!(sd_round_rect(50.0, 25.0, r, [8.0; 4]) < 0.0);
/// assert!((sd_round_rect(110.0, 25.0, r, [8.0; 4]) - 10.0).abs() < 1e-4);
/// ```
#[inline]
pub fn sd_round_rect(px: f32, py: f32, r: [f32; 4], radii: [f32; 4]) -> f32 {
    let hx = r[2] * 0.5;
    let hy = r[3] * 0.5;
    let cx = r[0] + hx;
    let cy = r[1] + hy;
    let dx = px - cx;
    let dy = py - cy;
    let rad = if dx > 0.0 {
        if dy < 0.0 { radii[1] } else { radii[2] }
    } else if dy < 0.0 {
        radii[0]
    } else {
        radii[3]
    };
    let rad = math::min(rad, math::min(hx, hy));
    let qx = math::abs(dx) - hx + rad;
    let qy = math::abs(dy) - hy + rad;
    let ox = math::max(qx, 0.0);
    let oy = math::max(qy, 0.0);
    math::sqrt(ox * ox + oy * oy) + math::min(math::max(qx, qy), 0.0) - rad
}

#[inline]
fn coverage(d: f32) -> f32 {
    math::clamp(0.5 - d, 0.0, 1.0)
}

fn erf2(x: f32) -> f32 {
    let s = if x < 0.0 { -1.0 } else { 1.0 };
    let a = math::abs(x);
    let mut t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    t *= t;
    s - s / (t * t)
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    math::exp(-(x * x) / (2.0 * sigma * sigma)) / (2.506_628_3 * sigma)
}

fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, hx: f32, hy: f32) -> f32 {
    let delta = math::min(hy - corner - math::abs(y), 0.0);
    let curved = hx - corner + math::sqrt(math::max(0.0, corner * corner - delta * delta));
    let k = 0.707_106_77 / sigma;
    let a = 0.5 + 0.5 * erf2((x - curved) * k);
    let b = 0.5 + 0.5 * erf2((x + curved) * k);
    b - a
}

/// Плотность размытой тени скруглённого прямоугольника в точке (метод
/// Эвана Уоллеса: точный интеграл по X, 4 выборки по Y).
///
/// ```
/// use orbita_ui::gfx::soft::shadow_density;
/// let inside = shadow_density(50.0, 50.0, [0.0, 0.0, 100.0, 100.0], 8.0, 4.0);
/// let far = shadow_density(200.0, 200.0, [0.0, 0.0, 100.0, 100.0], 8.0, 4.0);
/// assert!(inside > 0.95 && far < 0.01);
/// ```
pub fn shadow_density(px: f32, py: f32, shape: [f32; 4], corner: f32, sigma: f32) -> f32 {
    let hx = shape[2] * 0.5;
    let hy = shape[3] * 0.5;
    let x = px - (shape[0] + hx);
    let y = py - (shape[1] + hy);
    let corner = math::min(corner, math::min(hx, hy));
    let low = y - hy;
    let high = y + hy;
    let start = math::clamp(-3.0 * sigma, low, high);
    let end = math::clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    let mut yy = start + step * 0.5;
    let mut v = 0.0;
    for _ in 0..4 {
        v += shadow_x(x, y - yy, sigma, corner, hx, hy) * gaussian(yy, sigma) * step;
        yy += step;
    }
    v
}

/// Расстояние от точки до отрезка.
#[inline]
pub fn seg_distance(px: f32, py: f32, s: [f32; 4]) -> f32 {
    let (ax, ay, bx, by) = (s[0], s[1], s[2], s[3]);
    let (pax, pay) = (px - ax, py - ay);
    let (bax, bay) = (bx - ax, by - ay);
    let len2 = bax * bax + bay * bay;
    let h = if len2 > 0.0 { math::clamp((pax * bax + pay * bay) / len2, 0.0, 1.0) } else { 0.0 };
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    math::sqrt(dx * dx + dy * dy)
}

/// Статичный шум по координате пикселя (хеш), значение 0..1.
#[inline]
pub fn hash_noise(x: f32, y: f32) -> f32 {
    let xi = x as i32 as u32;
    let yi = y as i32 as u32;
    let mut h = xi.wrapping_mul(374_761_393) ^ yi.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

fn paint_at(inst: &Instance, px: f32, py: f32) -> Px {
    let kind = inst.grad0[0] as u32;
    if kind == 0 {
        return Px::premul(inst.color0);
    }
    let (t, mid, end) = if kind == 1 {
        let (x0, y0, x1, y1) = (inst.grad0[1], inst.grad0[2], inst.grad1[0], inst.grad1[1]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = math::max(dx * dx + dy * dy, 1e-6);
        (((px - x0) * dx + (py - y0) * dy) / len2, inst.grad0[3], inst.grad1[2])
    } else {
        let ux = (px - inst.grad0[1]) / inst.grad1[0];
        let uy = (py - inst.grad0[2]) / inst.grad1[1];
        (math::sqrt(ux * ux + uy * uy), inst.grad0[3], inst.grad1[2])
    };
    let c0 = Px::premul(inst.color0);
    let c1 = Px::premul(inst.color1);
    let c2 = Px::premul(inst.color2);
    let t = math::max(t, 0.0);
    if t <= mid {
        c0.mix(c1, if mid > 0.0 { t / mid } else { 1.0 })
    } else {
        let span = end - mid;
        c1.mix(c2, if span > 0.0 { math::clamp((t - mid) / span, 0.0, 1.0) } else { 1.0 })
    }
}

/// Цвет инстанса в пикселе (предумноженный), без учёта отсечения.
///
/// `backdrop` — функция выборки размытого заднего плана для стекла.
pub fn shade(
    inst: &Instance,
    segments: &[[f32; 4]],
    atlas: &Atlas,
    px: f32,
    py: f32,
    backdrop: &dyn Fn(f32, f32) -> Px,
) -> Px {
    let kind = inst.params[0] as u32;
    match kind {
        k if k == Kind::Rect as u32 => {
            let d = sd_round_rect(px, py, inst.rect, inst.radii);
            let cov = coverage(d);
            if cov <= 0.0 {
                return Px::default();
            }
            if inst.extra[3] > 0.0 {
                // Световая кромка: внутри фигуры, но вне её копии, сдвинутой вниз.
                let d2 = sd_round_rect(px, py - inst.extra[3], inst.rect, inst.radii);
                return Px::premul(inst.color0).scale(cov * (1.0 - coverage(d2)));
            }
            let fill = paint_at(inst, px, py).scale(cov);
            let bw = inst.params[1];
            if bw > 0.0 && inst.border[3] > 0.0 {
                let band = math::clamp(cov - coverage(d + bw), 0.0, 1.0);
                let b = Px::premul(inst.border).scale(band);
                b.over(fill)
            } else {
                fill
            }
        }
        k if k == Kind::Shadow as u32 => {
            let corner = math::max(math::max(inst.radii[0], inst.radii[1]), math::max(inst.radii[2], inst.radii[3]));
            let dens = shadow_density(px, py, inst.uv, corner, math::max(inst.params[3], 0.5));
            let el = [inst.grad0[0], inst.grad0[1], inst.grad0[2], inst.grad0[3]];
            let mask = if el[2] > 0.0 { 1.0 - coverage(sd_round_rect(px, py, el, inst.grad1)) } else { 1.0 };
            Px::premul(inst.color0).scale(dens * mask)
        }
        k if k == Kind::Glyph as u32 => {
            let u = inst.uv[0] + (px - inst.rect[0]) / inst.rect[2] * (inst.uv[2] - inst.uv[0]);
            let v = inst.uv[1] + (py - inst.rect[1]) / inst.rect[3] * (inst.uv[3] - inst.uv[1]);
            let ax = math::clamp(u * ATLAS_SIZE as f32, 0.0, (ATLAS_SIZE - 1) as f32) as u32;
            let ay = math::clamp(v * ATLAS_SIZE as f32, 0.0, (ATLAS_SIZE - 1) as f32) as u32;
            let c = atlas.sample(ax, ay) as f32 / 255.0;
            Px::premul(inst.color0).scale(c)
        }
        k if k == Kind::Path as u32 => {
            let (ox, oy, sc, mode) = (inst.uv[0], inst.uv[1], inst.uv[2], inst.uv[3]);
            let ix = (px - ox) / sc;
            let iy = (py - oy) / sc;
            let start = inst.seg[0] as usize;
            let count = inst.seg[1] as usize;
            let mut dmin = f32::MAX;
            let mut inside = false;
            // Отрезки дальше досягаемости штриха не влияют на расстояние —
            // для обводки их можно пропустить (большой выигрыш на обоях).
            let reach = (inst.params[3] * 0.5 + inst.extra[2] / sc + 2.0 / sc).max(0.0);
            for s in &segments[start..(start + count).min(segments.len())] {
                if mode <= 0.5 {
                    let (x0, x1) = if s[0] < s[2] { (s[0], s[2]) } else { (s[2], s[0]) };
                    let (y0, y1) = if s[1] < s[3] { (s[1], s[3]) } else { (s[3], s[1]) };
                    if ix < x0 - reach || ix > x1 + reach || iy < y0 - reach || iy > y1 + reach {
                        continue;
                    }
                }
                dmin = math::min(dmin, seg_distance(ix, iy, *s));
                if mode > 0.5 && ((s[1] > iy) != (s[3] > iy)) {
                    let xint = s[0] + (iy - s[1]) * (s[2] - s[0]) / (s[3] - s[1]);
                    if ix < xint {
                        inside = !inside;
                    }
                }
            }
            let d = dmin * sc;
            let soft = inst.extra[2];
            let a = if mode > 0.5 {
                let sd = if inside { -d } else { d };
                if soft > 0.0 { math::smoothstep(soft, -soft, sd) } else { coverage(sd) }
            } else {
                let hw = inst.params[3] * sc * 0.5;
                if soft > 0.0 { math::smoothstep(hw + soft, hw - soft, d) } else { math::clamp(hw - d + 0.5, 0.0, 1.0) }
            };
            if a <= 0.0 {
                return Px::default();
            }
            paint_at(inst, px, py).scale(a)
        }
        k if k == Kind::Backdrop as u32 => {
            let cov = coverage(sd_round_rect(px, py, inst.rect, inst.radii));
            if cov <= 0.0 {
                return Px::default();
            }
            let b = backdrop(px, py);
            let sat = inst.extra[1];
            let l = b.r * 0.2126 + b.g * 0.7152 + b.b * 0.0722;
            let c = Px {
                r: math::clamp(l + (b.r - l) * sat, 0.0, 1.0),
                g: math::clamp(l + (b.g - l) * sat, 0.0, 1.0),
                b: math::clamp(l + (b.b - l) * sat, 0.0, 1.0),
                a: 1.0,
            };
            c.scale(cov * inst.color0[3])
        }
        k if k == Kind::Noise as u32 => {
            let cov = coverage(sd_round_rect(px, py, inst.rect, inst.radii));
            if cov <= 0.0 {
                return Px::default();
            }
            let n = hash_noise(px, py) - 0.5;
            let a = math::abs(n) * inst.params[2] * 0.9 * cov;
            if n > 0.0 { Px { r: a, g: a, b: a, a } } else { Px { r: 0.0, g: 0.0, b: 0.0, a } }
        }
        _ => Px::default(),
    }
}

/// Покрытие отсечением инстанса в пикселе.
#[inline]
pub fn clip_coverage(inst: &Instance, px: f32, py: f32) -> f32 {
    let c = inst.clip;
    if px < c[0] || py < c[1] || px > c[2] || py > c[3] {
        return 0.0;
    }
    let r = inst.extra[0];
    if r <= 0.0 {
        return 1.0;
    }
    coverage(sd_round_rect(px, py, [c[0], c[1], c[2] - c[0], c[3] - c[1]], [r; 4]))
}

fn inst_bounds(inst: &Instance) -> Rect {
    let r = Rect::new(inst.rect[0], inst.rect[1], inst.rect[2], inst.rect[3]);
    let c = Rect::new(inst.clip[0], inst.clip[1], inst.clip[2] - inst.clip[0], inst.clip[3] - inst.clip[1]);
    r.intersect(&c)
}

fn inst_hash(inst: &Instance, segs: &[[f32; 4]]) -> u64 {
    // FNV-1a по битам всех полей (+ геометрия пути, если есть).
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let fields: [&[f32; 4]; 13] = [
        &inst.rect, &inst.radii, &inst.color0, &inst.color1, &inst.color2, &inst.border, &inst.params,
        &inst.grad0, &inst.grad1, &inst.clip, &inst.extra, &inst.uv, &inst.seg,
    ];
    for f in fields {
        for v in f {
            h ^= v.to_bits() as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    if inst.params[0] as u32 == Kind::Path as u32 {
        let s = inst.seg[0] as usize;
        let n = inst.seg[1] as usize;
        for seg in segs.iter().skip(s).take(n.min(8)) {
            for v in seg {
                h ^= v.to_bits() as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    h
}

/// Программный рендерер с отслеживанием повреждений.
pub struct SoftRenderer {
    width: u32,
    height: u32,
    prev: HashMap<u64, (u32, Rect)>,
    cache_key: u32,
    cache: Vec<u32>,
    blur: Vec<u32>,
    blur_w: u32,
    blur_h: u32,
    atlas_gen: u32,
    /// Принудительно перерисовать весь экран в следующем кадре.
    pub force_full: bool,
}

/// Во сколько раз уменьшается размытая копия обоев.
const BLUR_DOWN: u32 = 4;

impl SoftRenderer {
    /// Создаёт рендерер для экрана `width×height`.
    pub fn new(width: u32, height: u32) -> SoftRenderer {
        SoftRenderer {
            width,
            height,
            prev: HashMap::new(),
            cache_key: 0,
            cache: Vec::new(),
            blur: Vec::new(),
            blur_w: 0,
            blur_h: 0,
            atlas_gen: 0,
            force_full: true,
        }
    }

    /// Рисует кадр в буфер `target` (BGRA `0xAARRGGBB`, строка `stride` пикселей).
    ///
    /// Возвращает: список перерисованных прямоугольников — их и нужно
    /// отправить на экран (present). Пустой список — кадр не изменился.
    ///
    /// ```
    /// use orbita_ui::gfx::{Painter, Paint, soft::SoftRenderer};
    /// use orbita_ui::{Color, Rect};
    /// let mut p = Painter::new();
    /// p.begin_frame(64, 64, 1.0, Color::BLACK);
    /// p.rect(Rect::new(8.0, 8.0, 16.0, 16.0), 4.0, Paint::Solid(Color::WHITE));
    /// p.end_frame();
    /// let mut sr = SoftRenderer::new(64, 64);
    /// let mut buf = vec![0u32; 64 * 64];
    /// let dmg = sr.render(p.frame(), &p.fonts.atlas, &mut buf, 64);
    /// assert!(!dmg.is_empty());
    /// assert_eq!(buf[16 * 64 + 16], 0xffffffff);
    /// // Тот же кадр ещё раз — ничего не перерисовывается.
    /// assert!(sr.render(p.frame(), &p.fonts.atlas, &mut buf, 64).is_empty());
    /// ```
    pub fn render(&mut self, frame: &Frame, atlas: &Atlas, target: &mut [u32], stride: usize) -> Vec<Rect> {
        if frame.width != self.width || frame.height != self.height {
            *self = SoftRenderer::new(frame.width, frame.height);
        }
        if atlas.generation != self.atlas_gen {
            self.atlas_gen = atlas.generation;
            self.force_full = true;
        }
        let screen = Rect::new(0.0, 0.0, self.width as f32, self.height as f32);

        // 1. Кэш статичного пакета (обои) + размытая копия для стекла.
        let mut cached_batch: Option<(usize, usize)> = None;
        for b in &frame.batches {
            if b.cache_key != 0 {
                cached_batch = Some((b.start as usize, b.end as usize));
                if b.cache_key != self.cache_key || self.cache.len() != (self.width * self.height) as usize {
                    self.build_cache(frame, atlas, b.start as usize, b.end as usize);
                    self.cache_key = b.cache_key;
                    self.force_full = true;
                }
                break;
            }
        }

        // 2. Повреждения: инстансы, появившиеся или исчезнувшие с прошлого кадра.
        let mut cur: HashMap<u64, (u32, Rect)> = HashMap::with_capacity(frame.instances.len());
        for (i, inst) in frame.instances.iter().enumerate() {
            if let Some((s, e)) = cached_batch {
                if i >= s && i < e {
                    continue;
                }
            }
            let h = inst_hash(inst, &frame.segments);
            let e = cur.entry(h).or_insert((0, inst_bounds(inst)));
            e.0 += 1;
        }
        let mut damage: Vec<Rect> = Vec::new();
        if self.force_full {
            damage.push(screen);
        } else {
            for (h, (n, r)) in cur.iter() {
                match self.prev.get(h) {
                    Some((pn, _)) if pn == n => {}
                    _ => add_damage(&mut damage, *r),
                }
            }
            for (h, (n, r)) in self.prev.iter() {
                match cur.get(h) {
                    Some((cn, _)) if cn == n => {}
                    _ => add_damage(&mut damage, *r),
                }
            }
        }
        self.prev = cur;
        self.force_full = false;
        if damage.is_empty() {
            return damage;
        }
        let total: f32 = damage.iter().map(|r| r.w * r.h).sum();
        if total > screen.w * screen.h * 0.6 {
            damage.clear();
            damage.push(screen);
        }

        // 3. Перерисовка каждой области.
        let blur = (&self.blur, self.blur_w, self.blur_h);
        let bd = |x: f32, y: f32| -> Px { sample_blur(blur.0, blur.1, blur.2, x, y) };
        for d in damage.iter_mut() {
            *d = snap_out(d.intersect(&screen));
            let (x0, y0) = (d.x as u32, d.y as u32);
            let (x1, y1) = ((d.right() as u32).min(self.width), (d.bottom() as u32).min(self.height));
            // Фон: кэш обоев или цвет очистки.
            for y in y0..y1 {
                let row = (y as usize) * stride;
                if !self.cache.is_empty() && cached_batch.is_some() {
                    let src = (y * self.width) as usize;
                    target[row + x0 as usize..row + x1 as usize]
                        .copy_from_slice(&self.cache[src + x0 as usize..src + x1 as usize]);
                } else {
                    let c = pack(Px::premul(frame.clear));
                    target[row + x0 as usize..row + x1 as usize].fill(c);
                }
            }
            for (i, inst) in frame.instances.iter().enumerate() {
                if let Some((s, e)) = cached_batch {
                    if i >= s && i < e {
                        continue;
                    }
                }
                let b = inst_bounds(inst).intersect(d);
                if b.is_empty() {
                    continue;
                }
                draw_instance(inst, &frame.segments, atlas, b, target, stride, &bd);
            }
        }
        damage
    }

    fn build_cache(&mut self, frame: &Frame, atlas: &Atlas, start: usize, end: usize) {
        let (w, h) = (self.width, self.height);
        self.cache = vec![pack(Px::premul(frame.clear)); (w * h) as usize];
        let none = |_: f32, _: f32| Px::default();
        let full = Rect::new(0.0, 0.0, w as f32, h as f32);
        for inst in &frame.instances[start..end] {
            let b = inst_bounds(inst).intersect(&full);
            if !b.is_empty() {
                let b = snap_out(b);
                draw_instance(inst, &frame.segments, atlas, b, &mut self.cache, w as usize, &none);
            }
        }
        // Размытая копия: уменьшение в 4 раза + 3 прохода бокс-фильтра
        // (≈ гауссиан σ≈20px на исходном разрешении).
        let bw = (w / BLUR_DOWN).max(1);
        let bh = (h / BLUR_DOWN).max(1);
        let mut small = vec![[0f32; 3]; (bw * bh) as usize];
        for y in 0..bh {
            for x in 0..bw {
                let mut acc = [0f32; 3];
                for sy in 0..BLUR_DOWN {
                    for sx in 0..BLUR_DOWN {
                        let px = (x * BLUR_DOWN + sx).min(w - 1);
                        let py = (y * BLUR_DOWN + sy).min(h - 1);
                        let p = unpack(self.cache[(py * w + px) as usize]);
                        acc[0] += p.r;
                        acc[1] += p.g;
                        acc[2] += p.b;
                    }
                }
                let k = 1.0 / (BLUR_DOWN * BLUR_DOWN) as f32;
                small[(y * bw + x) as usize] = [acc[0] * k, acc[1] * k, acc[2] * k];
            }
        }
        for _ in 0..3 {
            box_blur(&mut small, bw as usize, bh as usize, 4);
        }
        self.blur = small.iter().map(|c| pack(Px { r: c[0], g: c[1], b: c[2], a: 1.0 })).collect();
        self.blur_w = bw;
        self.blur_h = bh;
    }
}

/// Растеризует диапазон инстансов `[start, end)` кадра в буфер `w×h`
/// (BGRA), предварительно залив его цветом очистки кадра.
///
/// Используется GPU-рендерером ядра, чтобы один раз подготовить текстуру
/// статичного пакета (обоев).
pub fn rasterize_range(frame: &Frame, atlas: &Atlas, start: usize, end: usize, target: &mut [u32], w: u32, h: u32) {
    let bg = pack(Px::premul(frame.clear));
    target.iter_mut().for_each(|p| *p = bg);
    let none = |_: f32, _: f32| Px::default();
    let full = Rect::new(0.0, 0.0, w as f32, h as f32);
    for inst in &frame.instances[start.min(frame.instances.len())..end.min(frame.instances.len())] {
        let b = inst_bounds(inst).intersect(&full);
        if !b.is_empty() {
            draw_instance(inst, &frame.segments, atlas, snap_out(b), target, w as usize, &none);
        }
    }
}

/// Растеризует один инстанс без отсечения в предумноженный BGRA-спрайт.
///
/// Принимает: инстанс, отрезки путей, атлас, левый верх спрайта в пикселях
/// экрана и его размер. Пиксели пишутся построчно в `out` (`w*h` слов,
/// `0xAARRGGBB` с предумноженными каналами).
pub fn rasterize_sprite(inst: &Instance, segs: &[[f32; 4]], atlas: &Atlas, x0: i32, y0: i32, w: u32, h: u32, out: &mut [u32]) {
    let none = |_: f32, _: f32| Px::default();
    for y in 0..h {
        for x in 0..w {
            let px = (x0 + x as i32) as f32 + 0.5;
            let py = (y0 + y as i32) as f32 + 0.5;
            let c = shade(inst, segs, atlas, px, py, &none);
            let q = |v: f32| (math::clamp(v, 0.0, 1.0) * 255.0 + 0.5) as u32;
            out[(y * w + x) as usize] = (q(c.a) << 24) | (q(c.r) << 16) | (q(c.g) << 8) | q(c.b);
        }
    }
}

fn box_blur(buf: &mut [[f32; 3]], w: usize, h: usize, r: usize) {
    let mut tmp = buf.to_vec();
    let k = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f32; 3];
            for dx in 0..=2 * r {
                let sx = (x + dx).saturating_sub(r).min(w - 1);
                let p = buf[y * w + sx];
                acc[0] += p[0];
                acc[1] += p[1];
                acc[2] += p[2];
            }
            tmp[y * w + x] = [acc[0] * k, acc[1] * k, acc[2] * k];
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f32; 3];
            for dy in 0..=2 * r {
                let sy = (y + dy).saturating_sub(r).min(h - 1);
                let p = tmp[sy * w + x];
                acc[0] += p[0];
                acc[1] += p[1];
                acc[2] += p[2];
            }
            buf[y * w + x] = [acc[0] * k, acc[1] * k, acc[2] * k];
        }
    }
}

fn sample_blur(buf: &[u32], w: u32, h: u32, x: f32, y: f32) -> Px {
    if buf.is_empty() {
        return Px { r: 0.12, g: 0.12, b: 0.12, a: 1.0 };
    }
    let fx = math::clamp(x / BLUR_DOWN as f32 - 0.5, 0.0, (w - 1) as f32);
    let fy = math::clamp(y / BLUR_DOWN as f32 - 0.5, 0.0, (h - 1) as f32);
    let x0 = fx as u32;
    let y0 = fy as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let a = unpack(buf[(y0 * w + x0) as usize]).mix(unpack(buf[(y0 * w + x1) as usize]), tx);
    let b = unpack(buf[(y1 * w + x0) as usize]).mix(unpack(buf[(y1 * w + x1) as usize]), tx);
    a.mix(b, ty)
}

fn snap_out(r: Rect) -> Rect {
    let x0 = math::floor(r.x);
    let y0 = math::floor(r.y);
    Rect::new(x0, y0, math::ceil(r.right()) - x0, math::ceil(r.bottom()) - y0)
}

fn add_damage(list: &mut Vec<Rect>, r: Rect) {
    if r.is_empty() {
        return;
    }
    let r = snap_out(r.inset(-1.0));
    // Сливаем с пересекающимися, чтобы не рисовать пиксели дважды.
    let mut merged = r;
    let mut i = 0;
    while i < list.len() {
        if !list[i].intersect(&merged.inset(-8.0)).is_empty() {
            merged = merged.union(&list[i]);
            list.swap_remove(i);
            i = 0;
            continue;
        }
        i += 1;
    }
    list.push(merged);
    if list.len() > 12 {
        let a = list.pop().unwrap();
        let b = list.pop().unwrap();
        list.push(a.union(&b));
    }
}

fn draw_instance(
    inst: &Instance,
    segs: &[[f32; 4]],
    atlas: &Atlas,
    area: Rect,
    target: &mut [u32],
    stride: usize,
    backdrop: &dyn Fn(f32, f32) -> Px,
) {
    let x0 = area.x as usize;
    let y0 = area.y as usize;
    let x1 = area.right() as usize;
    let y1 = area.bottom() as usize;
    let kind = inst.params[0] as u32;
    // Быстрый путь: сплошной прямоугольник без скругления, рамки и отсечения.
    let simple_rect = kind == Kind::Rect as u32
        && inst.grad0[0] == 0.0
        && inst.params[1] == 0.0
        && inst.extra[3] == 0.0
        && inst.extra[0] == 0.0
        && inst.radii == [0.0; 4]
        && inst.rect[0].fract_zero()
        && inst.rect[1].fract_zero();
    if simple_rect {
        let c = Px::premul(inst.color0);
        if c.a >= 0.999 {
            let v = pack(c);
            for y in y0..y1 {
                target[y * stride + x0..y * stride + x1].fill(v);
            }
            return;
        }
    }
    for y in y0..y1 {
        let py = y as f32 + 0.5;
        let row = y * stride;
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let clip = clip_coverage(inst, px, py);
            if clip <= 0.0 {
                continue;
            }
            let src = shade(inst, segs, atlas, px, py, backdrop).scale(clip);
            if src.a <= 0.0005 {
                continue;
            }
            let dst = unpack(target[row + x]);
            target[row + x] = pack(src.over(dst));
        }
    }
}

trait FractZero {
    fn fract_zero(self) -> bool;
}

impl FractZero for f32 {
    fn fract_zero(self) -> bool {
        math::floor(self) == self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::gfx::{GlassStyle, Paint, Painter};
    use crate::geom::Radii;

    #[test]
    fn rounded_corner_is_antialiased() {
        let mut p = Painter::new();
        p.begin_frame(32, 32, 1.0, Color::BLACK);
        p.rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radii::all(12.0), Paint::Solid(Color::WHITE));
        p.end_frame();
        let mut sr = SoftRenderer::new(32, 32);
        let mut buf = vec![0u32; 32 * 32];
        sr.render(p.frame(), &p.fonts.atlas, &mut buf, 32);
        assert_eq!(buf[0] & 0xffffff, 0x000000, "угол снаружи скругления");
        assert_eq!(buf[16 * 32 + 16], 0xffffffff, "центр залит");
    }

    #[test]
    fn damage_is_local_for_small_changes() {
        let mut sr = SoftRenderer::new(200, 200);
        let mut buf = vec![0u32; 200 * 200];
        let mut p = Painter::new();
        for x in [10.0, 20.0] {
            p.begin_frame(200, 200, 1.0, Color::BLACK);
            p.rect(Rect::new(100.0, 100.0, 50.0, 50.0), 0.0, Paint::Solid(Color::WHITE));
            p.rect(Rect::new(x, 10.0, 8.0, 8.0), 0.0, Paint::Solid(Color::WHITE));
            p.end_frame();
            let dmg = sr.render(p.frame(), &p.fonts.atlas, &mut buf, 200);
            if x > 10.0 {
                let area: f32 = dmg.iter().map(|r| r.w * r.h).sum();
                assert!(area < 1000.0, "перерисовано слишком много: {area}");
            }
        }
    }

    #[test]
    fn glass_samples_blurred_wallpaper() {
        let mut p = Painter::new();
        p.begin_frame(64, 64, 1.0, Color::BLACK);
        p.begin_cached(1);
        p.rect(Rect::new(0.0, 0.0, 64.0, 64.0), 0.0, Paint::Solid(Color::rgb_hex(0x2040ff)));
        p.end_cached();
        let style = GlassStyle {
            blur: 20.0,
            saturate: 1.0,
            fill: Color::TRANSPARENT,
            border: Color::TRANSPARENT,
            highlight: Color::TRANSPARENT,
            noise: 0.0,
        };
        p.glass(Rect::new(8.0, 8.0, 48.0, 48.0), Radii::all(8.0), &style, &[]);
        p.end_frame();
        let mut sr = SoftRenderer::new(64, 64);
        let mut buf = vec![0u32; 64 * 64];
        sr.render(p.frame(), &p.fonts.atlas, &mut buf, 64);
        let c = buf[32 * 64 + 32];
        assert!((c & 0xff) > 0xe0, "стекло должно показывать синий фон, получено {c:08x}");
    }
}
