//! Кодировщик потока команд virgl (`VIRGL_CCMD_*`).
//!
//! Каждая команда — заголовок `cmd | object << 8 | length << 16` и `length`
//! 32-битных слов полезной нагрузки. Раскладки полей повторяют Mesa
//! (`src/gallium/drivers/virgl/virgl_encode.c`) и virglrenderer
//! (`virgl_protocol.h`) — это и есть протокол, который исполняет QEMU.

extern crate alloc;

use alloc::vec::Vec;

/// Команды контекста.
pub mod ccmd {
    #![allow(missing_docs)]
    pub const CREATE_OBJECT: u32 = 1;
    pub const BIND_OBJECT: u32 = 2;
    pub const DESTROY_OBJECT: u32 = 3;
    pub const SET_VIEWPORT_STATE: u32 = 4;
    pub const SET_FRAMEBUFFER_STATE: u32 = 5;
    pub const SET_VERTEX_BUFFERS: u32 = 6;
    pub const CLEAR: u32 = 7;
    pub const DRAW_VBO: u32 = 8;
    pub const SET_SAMPLER_VIEWS: u32 = 10;
    pub const SET_INDEX_BUFFER: u32 = 11;
    pub const SET_CONSTANT_BUFFER: u32 = 12;
    pub const SET_SCISSOR_STATE: u32 = 15;
    pub const RESOURCE_INLINE_WRITE: u32 = 9;
    pub const RESOURCE_COPY_REGION: u32 = 17;
    pub const BIND_SAMPLER_STATES: u32 = 18;
    pub const SET_SUB_CTX: u32 = 28;
    pub const CREATE_SUB_CTX: u32 = 29;
    pub const BIND_SHADER: u32 = 31;
}

/// Типы объектов.
pub mod obj {
    #![allow(missing_docs)]
    pub const BLEND: u32 = 1;
    pub const RASTERIZER: u32 = 2;
    pub const DSA: u32 = 3;
    pub const SHADER: u32 = 4;
    pub const VERTEX_ELEMENTS: u32 = 5;
    pub const SAMPLER_VIEW: u32 = 6;
    pub const SAMPLER_STATE: u32 = 7;
    pub const SURFACE: u32 = 8;
}

/// Форматы virgl (`virgl_hw.h`).
pub mod fmt {
    /// BGRA 8 бит — формат экрана, текстур и атласа спрайтов.
    pub const B8G8R8A8_UNORM: u32 = 1;
    /// RGBA float32 — вершинные атрибуты.
    pub const R32G32B32A32_FLOAT: u32 = 31;
    /// Один канал 8 бит — атлас глифов.
    pub const R8_UNORM: u32 = 64;
    /// Индексы u32 (формат буфера не важен, используем R32_UINT-совместимый).
    pub const R32_UINT: u32 = 71;
}

/// Флаги привязки ресурсов (`VIRGL_BIND_*`).
pub mod bind {
    #![allow(missing_docs)]
    pub const RENDER_TARGET: u32 = 1 << 1;
    pub const SAMPLER_VIEW: u32 = 1 << 3;
    pub const VERTEX_BUFFER: u32 = 1 << 4;
    pub const INDEX_BUFFER: u32 = 1 << 5;
    pub const CONSTANT_BUFFER: u32 = 1 << 6;
    pub const SCANOUT: u32 = 1 << 18;
}

/// Стадия шейдера.
pub const SHADER_VERTEX: u32 = 0;
/// Фрагментная стадия.
pub const SHADER_FRAGMENT: u32 = 1;

/// Треугольники (`PIPE_PRIM_TRIANGLES`).
pub const PRIM_TRIANGLES: u32 = 4;

const fn cmd0(cmd: u32, object: u32, len: u32) -> u32 {
    cmd | (object << 8) | (len << 16)
}

/// Буфер команд virgl.
#[derive(Default, Clone)]
pub struct Encoder {
    /// Закодированные слова.
    pub buf: Vec<u32>,
}

impl Encoder {
    /// Пустой буфер.
    pub fn new() -> Encoder {
        Encoder { buf: Vec::new() }
    }

    /// Очищает буфер (память сохраняется).
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    fn head(&mut self, cmd: u32, object: u32, len: u32) {
        self.buf.push(cmd0(cmd, object, len));
    }

    fn f(&mut self, v: f32) {
        self.buf.push(v.to_bits());
    }

    /// Создаёт и делает текущим подконтекст `id`.
    pub fn sub_ctx(&mut self, id: u32) {
        self.head(ccmd::CREATE_SUB_CTX, 0, 1);
        self.buf.push(id);
        self.head(ccmd::SET_SUB_CTX, 0, 1);
        self.buf.push(id);
    }

    /// Состояние смешивания: предумноженная альфа (`ONE, INV_SRC_ALPHA`).
    ///
    /// ```
    /// let mut e = orbita_virgl::encoder::Encoder::new();
    /// e.blend_premultiplied(1);
    /// assert_eq!(e.buf.len(), 12); // заголовок + 11 слов
    /// assert_eq!(e.buf[0] >> 16, 11);
    /// ```
    pub fn blend_premultiplied(&mut self, handle: u32) {
        const ONE: u32 = 0x1;
        const INV_SRC_ALPHA: u32 = 0x13;
        const ADD: u32 = 0;
        self.head(ccmd::CREATE_OBJECT, obj::BLEND, 11);
        self.buf.push(handle);
        self.buf.push(0); // S0
        self.buf.push(0); // S1 logicop
        let rt = 1 | (ADD << 1) | (ONE << 4) | (INV_SRC_ALPHA << 9) | (ADD << 14) | (ONE << 17) | (INV_SRC_ALPHA << 22) | (0xF << 27);
        self.buf.push(rt);
        for _ in 1..8 {
            self.buf.push(0xF << 27);
        }
    }

    /// Смешивание выключено (копирование: даунсэмпл размытия).
    pub fn blend_opaque(&mut self, handle: u32) {
        self.head(ccmd::CREATE_OBJECT, obj::BLEND, 11);
        self.buf.push(handle);
        self.buf.push(0);
        self.buf.push(0);
        for _ in 0..8 {
            self.buf.push(0xF << 27);
        }
    }

    /// Растеризатор: без отсечения граней, заливка, центр пикселя 0.5.
    pub fn rasterizer(&mut self, handle: u32) {
        self.head(ccmd::CREATE_OBJECT, obj::RASTERIZER, 9);
        self.buf.push(handle);
        let s0 = (1 << 1) /* depth_clip */ | (1 << 29) /* half_pixel_center */;
        self.buf.push(s0);
        self.f(1.0); // point size
        self.buf.push(0); // sprite coord enable
        self.buf.push(0); // S3
        self.f(1.0); // line width
        self.f(0.0);
        self.f(0.0);
        self.f(0.0);
    }

    /// Глубина/трафарет/альфа-тест выключены.
    pub fn dsa_off(&mut self, handle: u32) {
        self.head(ccmd::CREATE_OBJECT, obj::DSA, 5);
        self.buf.push(handle);
        self.buf.push(0);
        self.buf.push(0);
        self.buf.push(0);
        self.buf.push(0);
    }

    /// Привязывает объект состояния (blend/rasterizer/dsa/vertex elements).
    pub fn bind_object(&mut self, object: u32, handle: u32) {
        self.head(ccmd::BIND_OBJECT, object, 1);
        self.buf.push(handle);
    }

    /// Удаляет объект (поверхность, представление текстуры и т.п.).
    pub fn destroy_object(&mut self, object: u32, handle: u32) {
        self.head(ccmd::DESTROY_OBJECT, object, 1);
        self.buf.push(handle);
    }

    /// Создаёт шейдер из текста TGSI.
    ///
    /// Принимает: handle, стадию ([`SHADER_VERTEX`]/[`SHADER_FRAGMENT`]),
    /// текст TGSI. Текст передаётся с завершающим нулём, дополненным до слова.
    ///
    /// ```
    /// let mut e = orbita_virgl::encoder::Encoder::new();
    /// e.shader(5, 1, "FRAG\nEND\n");
    /// assert_eq!(e.buf[1], 5);
    /// assert_eq!(e.buf[3], 10); // длина текста + NUL
    /// ```
    pub fn shader(&mut self, handle: u32, stage: u32, tgsi: &str) {
        let bytes = tgsi.as_bytes();
        let total = bytes.len() + 1;
        let words = total.div_ceil(4);
        self.head(ccmd::CREATE_OBJECT, obj::SHADER, 5 + words as u32);
        self.buf.push(handle);
        self.buf.push(stage);
        self.buf.push(total as u32 & 0x7fff_ffff); // offlen: полная длина
        self.buf.push(4096); // запас токенов для парсера хоста
        self.buf.push(0); // stream output
        let mut padded = Vec::with_capacity(words * 4);
        padded.extend_from_slice(bytes);
        padded.resize(words * 4, 0);
        for c in padded.chunks(4) {
            self.buf.push(u32::from_le_bytes([c[0], c[1], c[2], c[3]]));
        }
    }

    /// Делает шейдер активным.
    pub fn bind_shader(&mut self, handle: u32, stage: u32) {
        self.head(ccmd::BIND_SHADER, 0, 2);
        self.buf.push(handle);
        self.buf.push(stage);
    }

    /// Раскладка вершин: `n` атрибутов vec4 float подряд из буфера 0.
    pub fn vertex_elements_vec4(&mut self, handle: u32, n: u32) {
        self.head(ccmd::CREATE_OBJECT, obj::VERTEX_ELEMENTS, 4 * n + 1);
        self.buf.push(handle);
        for i in 0..n {
            self.buf.push(i * 16); // src_offset
            self.buf.push(0); // instance_divisor
            self.buf.push(0); // vertex_buffer_index
            self.buf.push(fmt::R32G32B32A32_FLOAT);
        }
    }

    /// Поверхность (цель рендера) уровня 0 текстуры `res`.
    pub fn surface(&mut self, handle: u32, res: u32, format: u32) {
        self.head(ccmd::CREATE_OBJECT, obj::SURFACE, 5);
        self.buf.push(handle);
        self.buf.push(res);
        self.buf.push(format);
        self.buf.push(0); // level
        self.buf.push(0); // first_layer | last_layer << 16
    }

    /// Представление текстуры для сэмплирования (swizzle по умолчанию).
    pub fn sampler_view(&mut self, handle: u32, res: u32, format: u32) {
        self.head(ccmd::CREATE_OBJECT, obj::SAMPLER_VIEW, 6);
        self.buf.push(handle);
        self.buf.push(res);
        self.buf.push(format);
        self.buf.push(0); // first_layer | last_layer << 16
        self.buf.push(0); // first_level | last_level << 8
        self.buf.push(0 | (1 << 3) | (2 << 6) | (3 << 9)); // XYZW
    }

    /// Сэмплер: зажим по краю, линейная фильтрация (`linear`=false — ближайший).
    pub fn sampler_state(&mut self, handle: u32, linear: bool) {
        const CLAMP_TO_EDGE: u32 = 2;
        let filt = linear as u32; // PIPE_TEX_FILTER_LINEAR = 1
        self.head(ccmd::CREATE_OBJECT, obj::SAMPLER_STATE, 9);
        self.buf.push(handle);
        let s0 = CLAMP_TO_EDGE | (CLAMP_TO_EDGE << 3) | (CLAMP_TO_EDGE << 6) | (filt << 9) | (2 << 11) /* mip none */ | (filt << 13);
        self.buf.push(s0);
        self.f(0.0);
        self.f(0.0);
        self.f(0.0);
        for _ in 0..4 {
            self.buf.push(0);
        }
    }

    /// Назначает представления текстур слотам `start..` стадии.
    pub fn set_sampler_views(&mut self, stage: u32, start: u32, handles: &[u32]) {
        self.head(ccmd::SET_SAMPLER_VIEWS, 0, 2 + handles.len() as u32);
        self.buf.push(stage);
        self.buf.push(start);
        self.buf.extend_from_slice(handles);
    }

    /// Назначает сэмплеры слотам `start..` стадии.
    pub fn bind_sampler_states(&mut self, stage: u32, start: u32, handles: &[u32]) {
        self.head(ccmd::BIND_SAMPLER_STATES, 0, 2 + handles.len() as u32);
        self.buf.push(stage);
        self.buf.push(start);
        self.buf.extend_from_slice(handles);
    }

    /// Константы стадии (буфер 0) — значения float.
    pub fn constants(&mut self, stage: u32, vals: &[f32]) {
        self.head(ccmd::SET_CONSTANT_BUFFER, 0, 2 + vals.len() as u32);
        self.buf.push(stage);
        self.buf.push(0);
        for v in vals {
            self.f(*v);
        }
    }

    /// Цель рендера: одна цветовая поверхность без глубины.
    pub fn framebuffer(&mut self, surface: u32) {
        self.head(ccmd::SET_FRAMEBUFFER_STATE, 0, 3);
        self.buf.push(1);
        self.buf.push(0);
        self.buf.push(surface);
    }

    /// Вьюпорт на всю цель `w×h`.
    pub fn viewport(&mut self, w: f32, h: f32) {
        self.head(ccmd::SET_VIEWPORT_STATE, 0, 7);
        self.buf.push(0);
        self.f(w * 0.5);
        self.f(h * 0.5);
        self.f(0.5);
        self.f(w * 0.5);
        self.f(h * 0.5);
        self.f(0.5);
    }

    /// Вершинный буфер 0: шаг `stride` байт, смещение, ресурс.
    pub fn vertex_buffer(&mut self, stride: u32, offset: u32, res: u32) {
        self.head(ccmd::SET_VERTEX_BUFFERS, 0, 3);
        self.buf.push(stride);
        self.buf.push(offset);
        self.buf.push(res);
    }

    /// Индексный буфер (u32).
    pub fn index_buffer(&mut self, res: u32) {
        self.head(ccmd::SET_INDEX_BUFFER, 0, 3);
        self.buf.push(res);
        self.buf.push(4);
        self.buf.push(0);
    }

    /// Очистка цвета.
    pub fn clear_color(&mut self, c: [f32; 4]) {
        self.head(ccmd::CLEAR, 0, 8);
        self.buf.push(4); // PIPE_CLEAR_COLOR0
        for v in c {
            self.f(v);
        }
        self.buf.push(0); // depth (double) lo
        self.buf.push(0); // depth hi
        self.buf.push(0); // stencil
    }

    /// Рисование треугольников: индексированное (`indexed`) или нет.
    pub fn draw(&mut self, start: u32, count: u32, indexed: bool) {
        self.head(ccmd::DRAW_VBO, 0, 12);
        self.buf.push(start);
        self.buf.push(count);
        self.buf.push(PRIM_TRIANGLES);
        self.buf.push(indexed as u32);
        self.buf.push(1); // instance_count
        self.buf.push(0); // index_bias
        self.buf.push(0); // start_instance
        self.buf.push(0); // primitive_restart
        self.buf.push(0); // restart_index
        self.buf.push(0); // min_index
        self.buf.push(0xffff_ffff); // max_index
        self.buf.push(0); // count_from_so
    }

    /// Записывает байты прямо в буфер-ресурс (данные внутри потока команд).
    ///
    /// Принимает: ресурс, смещение в байтах и данные (длина кратна 4).
    /// Большие данные режутся на куски по 32 КиБ (длина команды — 16 бит).
    pub fn inline_write(&mut self, res: u32, offset: u32, data: &[u8]) {
        let mut at = 0usize;
        while at < data.len() {
            let n = (data.len() - at).min(32 * 1024);
            let words = n.div_ceil(4);
            self.head(ccmd::RESOURCE_INLINE_WRITE, 0, 11 + words as u32);
            // res, level, usage, stride, layer_stride, x, y, z, w, h, d
            self.buf.extend_from_slice(&[res, 0, 0, 0, 0, offset + at as u32, 0, 0, n as u32, 1, 1]);
            for c in data[at..at + n].chunks(4) {
                let mut b = [0u8; 4];
                b[..c.len()].copy_from_slice(c);
                self.buf.push(u32::from_le_bytes(b));
            }
            at += n;
        }
    }

    /// Копирует область текстуры `src` в `dst` (уровень 0).
    pub fn copy_region(&mut self, dst: u32, dx: u32, dy: u32, src: u32, x: u32, y: u32, w: u32, h: u32) {
        self.head(ccmd::RESOURCE_COPY_REGION, 0, 13);
        self.buf.extend_from_slice(&[dst, 0, dx, dy, 0, src, 0, x, y, 0, w, h, 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout() {
        assert_eq!(cmd0(ccmd::DRAW_VBO, 0, 12), 8 | (12 << 16));
        assert_eq!(cmd0(ccmd::CREATE_OBJECT, obj::SHADER, 7), 1 | (4 << 8) | (7 << 16));
    }

    #[test]
    fn lengths_match_headers() {
        let mut e = Encoder::new();
        e.rasterizer(1);
        e.dsa_off(2);
        e.vertex_elements_vec4(3, 13);
        e.sampler_view(4, 9, fmt::R8_UNORM);
        e.sampler_state(5, true);
        e.draw(0, 6, true);
        e.copy_region(1, 0, 0, 2, 0, 0, 10, 10);
        e.shader(6, SHADER_FRAGMENT, "FRAG\nEND\n");
        // Проходим поток по заголовкам: длины должны сойтись точно.
        let mut i = 0;
        let mut n = 0;
        while i < e.buf.len() {
            i += 1 + (e.buf[i] >> 16) as usize;
            n += 1;
        }
        assert_eq!(i, e.buf.len());
        assert_eq!(n, 8);
    }
}
