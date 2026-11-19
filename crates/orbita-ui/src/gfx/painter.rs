//! `Painter` — API рисования, превращающее вызовы в инстансы кадра.
//!
//! Работает в логических пикселях (CSS px); сам переводит их в физические
//! через `scale`, применяет стек отсечений, групповой прозрачности и
//! трансформаций (масштаб/сдвиг для анимаций появления окон и меню).
//!
//! ```
//! use orbita_ui::gfx::{Painter, Paint};
//! use orbita_ui::{Color, Rect, Radii};
//! let mut p = Painter::new();
//! p.begin_frame(800, 600, 1.0, Color::BLACK);
//! p.rect(Rect::new(10.0, 10.0, 100.0, 40.0), Radii::all(12.0), Paint::Solid(Color::WHITE));
//! let frame = p.end_frame();
//! assert_eq!(frame.instances.len(), 1);
//! ```

extern crate alloc;

use alloc::vec::Vec;
use hashbrown::HashMap;

use super::text::{Face, Fonts};
use super::{Batch, Frame, Instance, Kind};
use crate::color::Color;
use crate::geom::{Radii, Rect, Vec2};
use crate::icons::{ICON_POINTS, Icon, IconPaint, LayerMode};
use crate::math;
use crate::tokens::{TextStyle, Theme};

/// Заливка фигуры.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// Сплошной цвет.
    Solid(Color),
    /// Линейный градиент как CSS `linear-gradient(<angle>deg, c0, c1 mid, c2)`.
    Linear {
        /// Угол CSS: 0° — снизу вверх, 90° — слева направо, 180° — сверху вниз.
        angle: f32,
        /// Цвет в начале.
        c0: Color,
        /// Цвет в средней точке.
        c1: Color,
        /// Цвет в конце.
        c2: Color,
        /// Позиция средней точки (0..1).
        mid: f32,
    },
    /// Радиальный градиент как CSS `radial-gradient(rx ry at cx cy, c0, c1 mid, c2 end)`.
    Radial {
        /// Центр X (доля ширины фигуры).
        cx: f32,
        /// Центр Y (доля высоты фигуры).
        cy: f32,
        /// Полуось X (доля ширины фигуры).
        rx: f32,
        /// Полуось Y (доля высоты фигуры).
        ry: f32,
        /// Цвет в центре.
        c0: Color,
        /// Цвет в средней точке.
        c1: Color,
        /// Цвет на краю.
        c2: Color,
        /// Позиция средней точки (0..1 радиуса).
        mid: f32,
        /// Позиция конечной точки (0..1 радиуса).
        end: f32,
    },
}

impl Paint {
    /// Двухточечный линейный градиент.
    ///
    /// ```
    /// use orbita_ui::{gfx::Paint, Color};
    /// let p = Paint::linear(145.0, Color::WHITE, Color::BLACK);
    /// assert!(matches!(p, Paint::Linear { .. }));
    /// ```
    pub fn linear(angle: f32, from: Color, to: Color) -> Paint {
        Paint::Linear { angle, c0: from, c1: to, c2: to, mid: 1.0 }
    }

    /// «Ambient-пятно»: цвет в центре, прозрачность на `end` радиуса.
    pub fn blob(cx: f32, cy: f32, rx: f32, ry: f32, color: Color, end: f32) -> Paint {
        let clear = color.with_alpha(0.0);
        Paint::Radial { cx, cy, rx, ry, c0: color, c1: clear, c2: clear, mid: end, end }
    }

    fn fade(self, k: f32) -> Paint {
        match self {
            Paint::Solid(c) => Paint::Solid(c.fade(k)),
            Paint::Linear { angle, c0, c1, c2, mid } => {
                Paint::Linear { angle, c0: c0.fade(k), c1: c1.fade(k), c2: c2.fade(k), mid }
            }
            Paint::Radial { cx, cy, rx, ry, c0, c1, c2, mid, end } => Paint::Radial {
                cx,
                cy,
                rx,
                ry,
                c0: c0.fade(k),
                c1: c1.fade(k),
                c2: c2.fade(k),
                mid,
                end,
            },
        }
    }

    fn is_invisible(&self) -> bool {
        match self {
            Paint::Solid(c) => c.a <= 0.002,
            Paint::Linear { c0, c1, c2, .. } | Paint::Radial { c0, c1, c2, .. } => {
                c0.a <= 0.002 && c1.a <= 0.002 && c2.a <= 0.002
            }
        }
    }
}

/// Тень `box-shadow: dx dy blur spread color`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Смещение по X.
    pub dx: f32,
    /// Смещение по Y.
    pub dy: f32,
    /// Радиус размытия CSS (σ = blur / 2).
    pub blur: f32,
    /// Расширение фигуры (может быть отрицательным).
    pub spread: f32,
    /// Цвет.
    pub color: Color,
}

impl Shadow {
    /// Конструктор в порядке CSS.
    ///
    /// ```
    /// let s = orbita_ui::gfx::Shadow::new(0.0, 24.0, 64.0, -8.0, orbita_ui::Color::BLACK.with_alpha(0.7));
    /// assert_eq!(s.spread, -8.0);
    /// ```
    pub const fn new(dx: f32, dy: f32, blur: f32, spread: f32, color: Color) -> Shadow {
        Shadow { dx, dy, blur, spread, color }
    }
}

/// Параметры стеклянной поверхности (`ob-glass`, `ob-window`, `ob-menu`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassStyle {
    /// Сила размытия заднего плана (px, токены `blur-*`); 0 — без стекла.
    pub blur: f32,
    /// Насыщенность заднего плана (1.65 / 1.8).
    pub saturate: f32,
    /// Заливка поверх размытого фона (`glass-fill*`).
    pub fill: Color,
    /// Цвет контура 1px (`glass-border` / `stroke-subtle`), прозрачный — без контура.
    pub border: Color,
    /// Световая кромка сверху (`inset 0 1px 0 glass-highlight`).
    pub highlight: Color,
    /// Сила шума (`noise-*`).
    pub noise: f32,
}

#[derive(Clone, Copy)]
struct Clip {
    rect: Rect,
    radius: f32,
}

#[derive(Clone, Copy)]
struct Xform {
    k: f32,
    tx: f32,
    ty: f32,
}

/// Рисовальщик кадра. Создаётся один раз и переиспользуется (буферы не
/// перевыделяются между кадрами).
pub struct Painter {
    frame: Frame,
    /// Система шрифтов и атлас глифов.
    pub fonts: Fonts,
    /// Масштаб экрана (физические px на логический).
    pub scale: f32,
    clips: Vec<Clip>,
    opacity: Vec<f32>,
    xforms: Vec<Xform>,
    batch_start: u32,
    batch_capture: bool,
    batch_cache: u32,
    icon_cache: HashMap<(u16, u16), (u32, u32)>,
    /// Логический размер экрана.
    pub screen: Rect,
}

impl Painter {
    /// Новый рисовальщик со встроенными шрифтами.
    pub fn new() -> Painter {
        Painter {
            frame: Frame::default(),
            fonts: Fonts::new(),
            scale: 1.0,
            clips: Vec::new(),
            opacity: Vec::new(),
            xforms: Vec::new(),
            batch_start: 0,
            batch_capture: false,
            batch_cache: 0,
            icon_cache: HashMap::new(),
            screen: Rect::ZERO,
        }
    }

    /// Начинает кадр.
    ///
    /// Принимает: размер цели в физических px, масштаб, цвет фона.
    pub fn begin_frame(&mut self, width: u32, height: u32, scale: f32, clear: Color) {
        self.scale = if scale > 0.0 { scale } else { 1.0 };
        self.frame.reset(width, height, clear.to_array());
        self.screen = Rect::new(0.0, 0.0, width as f32 / self.scale, height as f32 / self.scale);
        self.clips.clear();
        self.clips.push(Clip { rect: Rect::new(0.0, 0.0, width as f32, height as f32), radius: 0.0 });
        self.opacity.clear();
        self.opacity.push(1.0);
        self.xforms.clear();
        self.xforms.push(Xform { k: 1.0, tx: 0.0, ty: 0.0 });
        self.batch_start = 0;
        self.batch_capture = false;
        self.batch_cache = 0;
        self.icon_cache.clear();
    }

    /// Завершает кадр и отдаёт его рендереру.
    pub fn end_frame(&mut self) -> &Frame {
        self.close_batch();
        &self.frame
    }

    /// Текущий кадр (после `end_frame`).
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    fn close_batch(&mut self) {
        let end = self.frame.instances.len() as u32;
        if end > self.batch_start || self.batch_capture {
            self.frame.batches.push(Batch {
                start: self.batch_start,
                end,
                capture: self.batch_capture as u32,
                cache_key: self.batch_cache,
            });
        }
        self.batch_start = end;
        self.batch_capture = false;
        self.batch_cache = 0;
    }

    /// Начинает статичный пакет с ключом кэша (обои): рендерер может
    /// нарисовать его один раз и переиспользовать, пока ключ тот же.
    pub fn begin_cached(&mut self, key: u32) {
        self.close_batch();
        self.batch_cache = key;
    }

    /// Завершает статичный пакет.
    pub fn end_cached(&mut self) {
        self.close_batch();
    }

    // ------------------------------------------------------------ стеки

    /// Отсекает всё последующее прямоугольником (с опциональным скруглением).
    ///
    /// Обязательно парный вызов [`Painter::pop_clip`].
    pub fn push_clip(&mut self, r: Rect, radius: f32) {
        let dev = self.map_rect(r);
        let parent = *self.clips.last().unwrap();
        let rect = dev.intersect(&parent.rect);
        let radius = if radius > 0.0 { radius * self.cur_k() } else { parent.radius };
        self.clips.push(Clip { rect, radius });
    }

    /// Снимает отсечение.
    pub fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    /// Умножает прозрачность всего последующего на `a` (групповая прозрачность).
    pub fn push_opacity(&mut self, a: f32) {
        let cur = *self.opacity.last().unwrap();
        self.opacity.push(cur * math::clamp(a, 0.0, 1.0));
    }

    /// Снимает групповую прозрачность.
    pub fn pop_opacity(&mut self) {
        if self.opacity.len() > 1 {
            self.opacity.pop();
        }
    }

    /// Применяет масштаб `k` относительно точки `origin` и сдвиг `offset`
    /// ко всему последующему (анимации `ob-win`, `ob-pop`).
    pub fn push_transform(&mut self, origin: Vec2, k: f32, offset: Vec2) {
        let cur = *self.xforms.last().unwrap();
        // p' = origin + (p - origin)·k + offset, затем текущее преобразование.
        let tx = origin.x * (1.0 - k) + offset.x;
        let ty = origin.y * (1.0 - k) + offset.y;
        self.xforms.push(Xform { k: cur.k * k, tx: cur.k * tx + cur.tx, ty: cur.k * ty + cur.ty });
    }

    /// Снимает преобразование.
    pub fn pop_transform(&mut self) {
        if self.xforms.len() > 1 {
            self.xforms.pop();
        }
    }

    fn cur_k(&self) -> f32 {
        self.xforms.last().unwrap().k * self.scale
    }

    fn alpha(&self) -> f32 {
        *self.opacity.last().unwrap()
    }

    /// Логический прямоугольник → физический с учётом трансформаций.
    pub fn map_rect(&self, r: Rect) -> Rect {
        let x = self.xforms.last().unwrap();
        let s = self.scale;
        Rect::new((r.x * x.k + x.tx) * s, (r.y * x.k + x.ty) * s, r.w * x.k * s, r.h * x.k * s)
    }

    /// Логическая точка → физическая.
    pub fn map_point(&self, p: Vec2) -> Vec2 {
        let x = self.xforms.last().unwrap();
        Vec2::new((p.x * x.k + x.tx) * self.scale, (p.y * x.k + x.ty) * self.scale)
    }

    fn base(&self, kind: Kind, dev: Rect) -> Option<Instance> {
        let clip = self.clips.last().unwrap();
        // Отбрасываем невидимое: ни одного пикселя внутри отсечения.
        let vis = dev.intersect(&clip.rect);
        if vis.is_empty() {
            return None;
        }
        let mut inst = Instance::default();
        inst.rect = [dev.x, dev.y, dev.w, dev.h];
        inst.params[0] = kind as u32 as f32;
        inst.clip = [clip.rect.x, clip.rect.y, clip.rect.right(), clip.rect.bottom()];
        inst.extra[0] = clip.radius;
        inst.extra[1] = 1.0;
        Some(inst)
    }

    fn set_paint(&self, inst: &mut Instance, paint: Paint, dev: Rect) {
        match paint {
            Paint::Solid(c) => {
                inst.color0 = c.to_array();
            }
            Paint::Linear { angle, c0, c1, c2, mid } => {
                let a = angle.to_radians_f();
                let (sx, cy) = (math::sin(a), math::cos(a));
                let half = (math::abs(dev.w * sx) + math::abs(dev.h * cy)) * 0.5;
                let c = dev.center();
                let dir = Vec2::new(sx, -cy);
                let p0 = c.sub(dir.scale(half));
                let p1 = c.add(dir.scale(half));
                inst.color0 = c0.to_array();
                inst.color1 = c1.to_array();
                inst.color2 = c2.to_array();
                inst.grad0 = [1.0, p0.x, p0.y, mid];
                inst.grad1 = [p1.x, p1.y, 1.0, 0.0];
            }
            Paint::Radial { cx, cy, rx, ry, c0, c1, c2, mid, end } => {
                inst.color0 = c0.to_array();
                inst.color1 = c1.to_array();
                inst.color2 = c2.to_array();
                inst.grad0 = [2.0, dev.x + dev.w * cx, dev.y + dev.h * cy, mid];
                inst.grad1 = [math::max(dev.w * rx, 0.5), math::max(dev.h * ry, 0.5), end, 0.0];
            }
        }
    }

    // ------------------------------------------------------------ фигуры

    /// Скруглённый прямоугольник с заливкой.
    ///
    /// Принимает: прямоугольник, радиусы, заливку.
    pub fn rect(&mut self, r: Rect, radii: impl Into<Radii>, paint: Paint) {
        self.rect_ex(r, radii.into(), paint, 0.0, Color::TRANSPARENT);
    }

    /// Скруглённый прямоугольник с заливкой и внутренней рамкой.
    ///
    /// Принимает: прямоугольник, радиусы, заливку, ширину рамки (лог. px),
    /// цвет рамки. Рамка рисуется внутрь фигуры (как `box-sizing: border-box`).
    pub fn rect_ex(&mut self, r: Rect, radii: Radii, paint: Paint, border_w: f32, border: Color) {
        let k = self.alpha();
        let paint = paint.fade(k);
        let border = border.fade(k);
        if paint.is_invisible() && (border_w <= 0.0 || border.a <= 0.002) {
            return;
        }
        let dev = self.map_rect(r);
        let Some(mut inst) = self.base(Kind::Rect, dev) else { return };
        let ck = self.cur_k();
        inst.radii = radii.clamp_to(r.w, r.h).scale(ck).to_array();
        self.set_paint(&mut inst, paint, dev);
        inst.params[1] = border_w * ck;
        inst.border = border.to_array();
        self.frame.instances.push(inst);
    }

    /// Только рамка (без заливки).
    pub fn stroke_rect(&mut self, r: Rect, radii: impl Into<Radii>, width: f32, color: Color) {
        self.rect_ex(r, radii.into(), Paint::Solid(Color::TRANSPARENT), width, color);
    }

    /// Световая кромка `box-shadow: inset 0 1px 0 color` по верхнему краю фигуры.
    pub fn inset_highlight(&mut self, r: Rect, radii: impl Into<Radii>, color: Color) {
        let color = color.fade(self.alpha());
        if color.a <= 0.002 {
            return;
        }
        let radii = radii.into();
        let dev = self.map_rect(r);
        let Some(mut inst) = self.base(Kind::Rect, dev) else { return };
        let ck = self.cur_k();
        inst.radii = radii.clamp_to(r.w, r.h).scale(ck).to_array();
        inst.color0 = color.to_array();
        inst.extra[3] = 1.0 * ck; // режим «внутренняя кромка», смещение 1px
        self.frame.instances.push(inst);
    }

    /// Размытая тень фигуры.
    pub fn shadow(&mut self, r: Rect, radii: impl Into<Radii>, s: Shadow) {
        let color = s.color.fade(self.alpha());
        if color.a <= 0.002 {
            return;
        }
        let radii = radii.into();
        let shape = Rect::new(r.x + s.dx - s.spread, r.y + s.dy - s.spread, r.w + 2.0 * s.spread, r.h + 2.0 * s.spread);
        if shape.w <= 0.0 || shape.h <= 0.0 {
            return;
        }
        let sigma = math::max(s.blur * 0.5, 0.5);
        let dev_shape = self.map_rect(shape);
        let ck = self.cur_k();
        let pad = sigma * 3.0 * ck;
        let quad = Rect::new(dev_shape.x - pad, dev_shape.y - pad, dev_shape.w + 2.0 * pad, dev_shape.h + 2.0 * pad);
        let Some(mut inst) = self.base(Kind::Shadow, quad) else { return };
        let grow = s.spread;
        let rr = Radii {
            tl: math::max(radii.tl + grow, 0.0),
            tr: math::max(radii.tr + grow, 0.0),
            br: math::max(radii.br + grow, 0.0),
            bl: math::max(radii.bl + grow, 0.0),
        };
        inst.radii = rr.clamp_to(shape.w, shape.h).scale(ck).to_array();
        inst.color0 = color.to_array();
        inst.params[3] = sigma * ck;
        inst.uv = [dev_shape.x, dev_shape.y, dev_shape.w, dev_shape.h];
        // Как в CSS, внешняя тень не рисуется под самим элементом
        // (иначе полупрозрачное стекло темнело бы изнутри): передаём
        // фигуру элемента для маски.
        let el = self.map_rect(r);
        inst.grad0 = [el.x, el.y, el.w, el.h];
        inst.grad1 = radii.clamp_to(r.w, r.h).scale(ck).to_array();
        self.frame.instances.push(inst);
    }

    /// Зерно шума поверх фигуры (имитация `mix-blend-mode: overlay` SVG-шума).
    pub fn noise(&mut self, r: Rect, radii: impl Into<Radii>, amount: f32) {
        let amount = amount * self.alpha();
        if amount <= 0.001 {
            return;
        }
        let radii = radii.into();
        let dev = self.map_rect(r);
        let Some(mut inst) = self.base(Kind::Noise, dev) else { return };
        inst.radii = radii.clamp_to(r.w, r.h).scale(self.cur_k()).to_array();
        inst.params[2] = amount;
        self.frame.instances.push(inst);
    }

    /// Размытый задний план в форме фигуры (backdrop-filter: blur + saturate).
    ///
    /// Закрывает текущий пакет: рендерер снимет копию нарисованного и
    /// построит пирамиду размытия перед следующим пакетом.
    pub fn backdrop(&mut self, r: Rect, radii: impl Into<Radii>, blur: f32, saturate: f32) {
        let a = self.alpha();
        if blur <= 0.0 || a <= 0.002 {
            return;
        }
        let radii = radii.into();
        let dev = self.map_rect(r);
        let Some(mut inst) = self.base(Kind::Backdrop, dev) else { return };
        // Снимок нужен, только если с прошлого снимка что-то нарисовано.
        let drawn_since = self.frame.instances.len() as u32 > self.batch_start;
        if drawn_since || self.frame.batches.is_empty() {
            self.close_batch();
            self.batch_capture = true;
        }
        let ck = self.cur_k();
        inst.radii = radii.clamp_to(r.w, r.h).scale(ck).to_array();
        inst.params[3] = blur * self.scale;
        inst.color0 = [1.0, 1.0, 1.0, a];
        inst.extra[1] = saturate;
        self.frame.instances.push(inst);
    }

    /// Стеклянная поверхность целиком: тени, размытие, заливка, шум, контур
    /// и световая кромка — в порядке, как их складывает браузер.
    pub fn glass(&mut self, r: Rect, radii: impl Into<Radii>, style: &GlassStyle, shadows: &[Shadow]) {
        let radii = radii.into();
        for s in shadows {
            self.shadow(r, radii, *s);
        }
        self.backdrop(r, radii, style.blur, style.saturate);
        self.rect(r, radii, Paint::Solid(style.fill));
        self.noise(r, radii, style.noise);
        if style.border.a > 0.0 {
            self.stroke_rect(r, radii, 1.0, style.border);
        }
        self.inset_highlight(r, radii, style.highlight);
    }

    // ------------------------------------------------------------ текст

    /// Рисует строку; `pos` — левый верх строки высотой `style.line`.
    ///
    /// Возвращает: ширину строки (логические px).
    pub fn text(&mut self, pos: Vec2, style: &TextStyle, text: &str, color: Color) -> f32 {
        let color = color.fade(self.alpha());
        let k = self.xforms.last().unwrap().k;
        let st = TextStyle { size: style.size * k, line: style.line * k, tracking: style.tracking * k, face: style.face };
        let baseline_l = self.fonts.baseline(&st);
        let origin = self.map_point(pos);
        let clip = *self.clips.last().unwrap();
        let mut instances = core::mem::take(&mut self.frame.instances);
        let adv = self.fonts.layout(&st, text, origin.x, origin.y + baseline_l * self.scale, self.scale, |q| {
            let r = Rect::new(q.rect[0], q.rect[1], q.rect[2], q.rect[3]);
            if r.intersect(&clip.rect).is_empty() || color.a <= 0.002 {
                return;
            }
            let mut inst = Instance::default();
            inst.rect = q.rect;
            inst.uv = q.uv;
            inst.color0 = color.to_array();
            inst.params[0] = Kind::Glyph as u32 as f32;
            inst.clip = [clip.rect.x, clip.rect.y, clip.rect.right(), clip.rect.bottom()];
            inst.extra[0] = clip.radius;
            inst.extra[1] = 1.0;
            instances.push(inst);
        });
        self.frame.instances = instances;
        adv / (self.scale * k)
    }

    /// Ширина строки в логических px.
    pub fn measure(&self, style: &TextStyle, text: &str) -> f32 {
        self.fonts.measure(style, text, self.scale)
    }

    /// Текст, выровненный по горизонтали внутри `r` (0 — влево, 0.5 — центр,
    /// 1 — вправо) и по вертикали по центру. Длинный текст обрезается «…».
    pub fn text_in(&mut self, r: Rect, style: &TextStyle, text: &str, color: Color, align: f32) -> f32 {
        let w = self.measure(style, text);
        let y = r.y + (r.h - style.line) * 0.5;
        if w <= r.w + 0.5 {
            let x = r.x + (r.w - w) * align;
            self.text(Vec2::new(x, y), style, text, color)
        } else {
            let fitted = self.ellipsize(style, text, r.w);
            self.text(Vec2::new(r.x, y), style, &fitted, color)
        }
    }

    /// Обрезает строку, добавляя «…», чтобы ширина не превышала `max_w`.
    ///
    /// ```
    /// let p = orbita_ui::gfx::Painter::new();
    /// let s = p.ellipsize(&orbita_ui::tokens::text::BODY_MD, "очень длинное имя файла", 60.0);
    /// assert!(s.ends_with('…'));
    /// ```
    pub fn ellipsize(&self, style: &TextStyle, text: &str, max_w: f32) -> alloc::string::String {
        if self.measure(style, text) <= max_w {
            return alloc::string::String::from(text);
        }
        let mut s = alloc::string::String::new();
        for ch in text.chars() {
            s.push(ch);
            s.push('…');
            if self.measure(style, &s) > max_w {
                s.pop();
                s.pop();
                break;
            }
            s.pop();
        }
        s.push('…');
        s
    }

    /// Начертание по умолчанию для проверок.
    pub fn default_face(&self) -> Face {
        Face::Sans400
    }

    // ------------------------------------------------------------ иконки

    /// Векторная иконка дизайн-системы в квадрате `r`.
    ///
    /// Принимает: иконку, квадрат, цвет `currentColor`, тему (для токенов
    /// внутри многоцветных иконок) и множитель толщины штриха (1 — как в
    /// макете: 1.75 на сетке 24px).
    pub fn icon(&mut self, icon: Icon, r: Rect, color: Color, theme: &Theme, stroke_k: f32) {
        let a = self.alpha();
        let def = icon.def();
        let dev = self.map_rect(r);
        let scale = dev.w / def.view;
        for (li, layer) in def.layers.iter().enumerate() {
            let (start, count) = self.icon_segments(icon as u16, li as u16, layer.mode == LayerMode::Fill);
            if count == 0 {
                continue;
            }
            // Габарит слоя (для градиентов objectBoundingBox).
            let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for &(s, e) in layer.contours {
                for p in &ICON_POINTS[s..e] {
                    minx = math::min(minx, p[0]);
                    miny = math::min(miny, p[1]);
                    maxx = math::max(maxx, p[0]);
                    maxy = math::max(maxy, p[1]);
                }
            }
            let hw = if layer.mode == LayerMode::Stroke { layer.width * stroke_k * 0.5 } else { 0.0 };
            let bb = Rect::new(
                dev.x + (minx - hw - 1.0) * scale,
                dev.y + (miny - hw - 1.0) * scale,
                (maxx - minx + 2.0 * hw + 2.0) * scale,
                (maxy - miny + 2.0 * hw + 2.0) * scale,
            );
            let Some(mut inst) = self.base(Kind::Path, bb) else { continue };
            let grad_box = Rect::new(dev.x + minx * scale, dev.y + miny * scale, (maxx - minx) * scale, (maxy - miny) * scale);
            let op = layer.opacity * a;
            let paint = match layer.paint {
                IconPaint::Solid(c) => Paint::Solid(c.resolve(theme, color).fade(op)),
                IconPaint::Linear(c0, c1) => {
                    Paint::linear(135.0, c0.resolve(theme, color).fade(op), c1.resolve(theme, color).fade(op))
                }
                IconPaint::Radial(c0, c1, c2) => Paint::Radial {
                    cx: 0.35,
                    cy: 0.3,
                    rx: 0.8,
                    ry: 0.8,
                    c0: c0.resolve(theme, color).fade(op),
                    c1: c1.resolve(theme, color).fade(op),
                    c2: c2.resolve(theme, color).fade(op),
                    mid: 0.35,
                    end: 1.0,
                },
            };
            self.set_paint(&mut inst, paint, grad_box);
            inst.params[3] = layer.width * stroke_k;
            inst.uv = [dev.x, dev.y, scale, if layer.mode == LayerMode::Fill { 1.0 } else { 0.0 }];
            inst.seg = [start as f32, count as f32, 0.0, 0.0];
            inst.extra[2] = 0.0;
            self.frame.instances.push(inst);
        }
    }

    fn icon_segments(&mut self, icon: u16, layer: u16, close: bool) -> (u32, u32) {
        if let Some(v) = self.icon_cache.get(&(icon, layer)) {
            return *v;
        }
        let def = Icon::ALL[icon as usize].def();
        let l = &def.layers[layer as usize];
        let start = self.frame.segments.len() as u32;
        for &(s, e) in l.contours {
            let pts = &ICON_POINTS[s..e];
            for w in pts.windows(2) {
                self.frame.segments.push([w[0][0], w[0][1], w[1][0], w[1][1]]);
            }
            if close {
                let (a, b) = (pts[pts.len() - 1], pts[0]);
                if a != b {
                    self.frame.segments.push([a[0], a[1], b[0], b[1]]);
                }
            }
        }
        let v = (start, self.frame.segments.len() as u32 - start);
        self.icon_cache.insert((icon, layer), v);
        v
    }

    /// Произвольная ломаная (обои: волны, кольца) в логических координатах.
    ///
    /// Принимает: точки, замкнута ли, заливка/обводка, толщину, мягкость
    /// края (px, имитация `filter: blur`) и заливку.
    pub fn polyline(&mut self, pts: &[Vec2], fill: bool, width: f32, soft: f32, paint: Paint, bounds_for_paint: Rect) {
        if pts.len() < 2 {
            return;
        }
        let a = self.alpha();
        let paint = paint.fade(a);
        if paint.is_invisible() {
            return;
        }
        let start = self.frame.segments.len() as u32;
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut prev = self.map_point(pts[0]);
        for p in pts.iter().skip(1) {
            let d = self.map_point(*p);
            self.frame.segments.push([prev.x, prev.y, d.x, d.y]);
            minx = math::min(minx, math::min(prev.x, d.x));
            miny = math::min(miny, math::min(prev.y, d.y));
            maxx = math::max(maxx, math::max(prev.x, d.x));
            maxy = math::max(maxy, math::max(prev.y, d.y));
            prev = d;
        }
        if fill {
            let first = self.map_point(pts[0]);
            self.frame.segments.push([prev.x, prev.y, first.x, first.y]);
        }
        let count = self.frame.segments.len() as u32 - start;
        let ck = self.cur_k();
        let pad = (width * 0.5 + soft) * ck + 2.0;
        let bb = Rect::new(minx - pad, miny - pad, maxx - minx + 2.0 * pad, maxy - miny + 2.0 * pad);
        let Some(mut inst) = self.base(Kind::Path, bb) else {
            self.frame.segments.truncate(start as usize);
            return;
        };
        let pb = self.map_rect(bounds_for_paint);
        self.set_paint(&mut inst, paint, pb);
        inst.params[3] = width * ck;
        inst.uv = [0.0, 0.0, 1.0, if fill { 1.0 } else { 0.0 }];
        inst.seg = [start as f32, count as f32, 0.0, 0.0];
        inst.extra[2] = soft * ck;
        self.frame.instances.push(inst);
    }

    /// Количество инстансов, накопленных в кадре (для диагностики).
    pub fn instance_count(&self) -> usize {
        self.frame.instances.len()
    }
}

impl Default for Painter {
    fn default() -> Self {
        Painter::new()
    }
}

trait ToRadians {
    fn to_radians_f(self) -> f32;
}

impl ToRadians for f32 {
    fn to_radians_f(self) -> f32 {
        self * core::f32::consts::PI / 180.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_requests_capture() {
        let mut p = Painter::new();
        p.begin_frame(400, 300, 1.0, Color::BLACK);
        p.rect(Rect::new(0.0, 0.0, 400.0, 300.0), 0.0, Paint::Solid(Color::WHITE));
        let style = GlassStyle {
            blur: 20.0,
            saturate: 1.8,
            fill: Color::WHITE.with_alpha(0.1),
            border: Color::TRANSPARENT,
            highlight: Color::WHITE.with_alpha(0.2),
            noise: 0.1,
        };
        p.glass(Rect::new(10.0, 10.0, 100.0, 100.0), Radii::all(16.0), &style, &[]);
        let f = p.end_frame();
        assert_eq!(f.batches.len(), 2);
        assert_eq!(f.batches[1].capture, 1);
    }

    #[test]
    fn clipped_out_instances_are_dropped() {
        let mut p = Painter::new();
        p.begin_frame(100, 100, 1.0, Color::BLACK);
        p.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0);
        p.rect(Rect::new(50.0, 50.0, 10.0, 10.0), 0.0, Paint::Solid(Color::WHITE));
        p.pop_clip();
        assert_eq!(p.end_frame().instances.len(), 0);
    }

    #[test]
    fn transform_scales_about_origin() {
        let mut p = Painter::new();
        p.begin_frame(100, 100, 2.0, Color::BLACK);
        p.push_transform(Vec2::new(10.0, 10.0), 0.5, Vec2::ZERO);
        let r = p.map_rect(Rect::new(0.0, 0.0, 20.0, 20.0));
        assert_eq!(r, Rect::new(10.0, 10.0, 20.0, 20.0));
    }

    #[test]
    fn icons_emit_paths() {
        let mut p = Painter::new();
        p.begin_frame(100, 100, 1.0, Color::BLACK);
        p.icon(Icon::IFolder, Rect::new(0.0, 0.0, 18.0, 18.0), Color::WHITE, &crate::tokens::DARK, 1.0);
        let f = p.end_frame();
        assert_eq!(f.instances.len(), 1);
        assert!(f.segments.len() > 8);
    }
}
