//! Текст: шрифты Onest и JetBrains Mono, атлас глифов, измерение и перенос.
//!
//! Шрифты — вариативные TTF из `assets/fonts` (OFL), встроены в бинарник
//! через `include_bytes!`, поэтому одинаково работают и в ядре (без ФС), и
//! в эмуляторе. Растеризация — `ab_glyph` (no_std), покрытие кладётся в
//! общий атлас R8 2048×2048, который рендерер загружает на GPU только по
//! «грязной» области.
//!
//! Позиционирование субпиксельное по X (4 фазы) — текст не «дрожит» при
//! плавных анимациях перемещения окна.

extern crate alloc;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, VariableFont, point};
use alloc::vec;
use alloc::vec::Vec;
use hashbrown::HashMap;

use crate::math;
use crate::tokens::TextStyle;

/// Вариативный шрифт интерфейса Onest.
pub static ONEST_TTF: &[u8] = include_bytes!("../../../../assets/fonts/Onest-Variable.ttf");
/// Вариативный моноширинный шрифт JetBrains Mono.
pub static JETBRAINS_MONO_TTF: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Variable.ttf");

/// Начертание: семейство + насыщенность (ось `wght` вариативного шрифта).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Face {
    /// Onest 400 — основной текст.
    Sans400 = 0,
    /// Onest 500 — кнопки, меню, подписи.
    Sans500 = 1,
    /// Onest 600 — заголовки.
    Sans600 = 2,
    /// Onest 700 — акценты, заставка.
    Sans700 = 3,
    /// JetBrains Mono 400 — терминал, код, пути.
    Mono400 = 4,
    /// JetBrains Mono 700 — полужирный в терминале.
    Mono700 = 5,
}

impl Face {
    const ALL: [Face; 6] = [Face::Sans400, Face::Sans500, Face::Sans600, Face::Sans700, Face::Mono400, Face::Mono700];

    fn weight(self) -> f32 {
        match self {
            Face::Sans400 | Face::Mono400 => 400.0,
            Face::Sans500 => 500.0,
            Face::Sans600 => 600.0,
            Face::Sans700 | Face::Mono700 => 700.0,
        }
    }

    fn is_mono(self) -> bool {
        matches!(self, Face::Mono400 | Face::Mono700)
    }
}

/// Сторона атласа глифов в пикселях.
pub const ATLAS_SIZE: u32 = 2048;

/// Положение глифа в атласе и его смещение от точки пера.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphEntry {
    /// Левый верхний угол в атласе (px).
    pub ax: u16,
    /// Верх в атласе.
    pub ay: u16,
    /// Ширина растра.
    pub w: u16,
    /// Высота растра.
    pub h: u16,
    /// Смещение растра от точки пера по X (px, может быть отрицательным).
    pub ox: f32,
    /// Смещение растра от базовой линии по Y (px).
    pub oy: f32,
}

/// Глиф, готовый к отрисовке: квад на экране + uv в атласе.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphQuad {
    /// Квад на экране `[x, y, w, h]` (физические px).
    pub rect: [f32; 4],
    /// uv в атласе `[u0, v0, u1, v1]` (0..1).
    pub uv: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: u8,
    size_q: u16,
    glyph: u16,
    sub: u8,
}

struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

/// Атлас покрытия глифов (R8) с полочной упаковкой.
pub struct Atlas {
    /// Пиксели атласа, строка за строкой, `ATLAS_SIZE²` байт.
    pub pixels: Vec<u8>,
    shelves: Vec<Shelf>,
    next_y: u32,
    dirty: Option<[u32; 4]>,
    /// Поколение: увеличивается при полной очистке (рендерер перезаливает всё).
    pub generation: u32,
}

impl Atlas {
    fn new() -> Atlas {
        Atlas {
            pixels: vec![0u8; (ATLAS_SIZE * ATLAS_SIZE) as usize],
            shelves: Vec::new(),
            next_y: 0,
            dirty: None,
            generation: 1,
        }
    }

    fn clear(&mut self) {
        self.pixels.iter_mut().for_each(|p| *p = 0);
        self.shelves.clear();
        self.next_y = 0;
        self.generation = self.generation.wrapping_add(1);
        self.dirty = Some([0, 0, ATLAS_SIZE, ATLAS_SIZE]);
    }

    /// Выделяет место под растр `w×h` (+1px поля от «протекания» билинейки).
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w, h) = (w + 1, h + 1);
        for shelf in self.shelves.iter_mut() {
            if h <= shelf.h && shelf.x + w <= ATLAS_SIZE && h * 4 >= shelf.h * 3 {
                let pos = (shelf.x, shelf.y);
                shelf.x += w;
                return Some(pos);
            }
        }
        if self.next_y + h > ATLAS_SIZE {
            return None;
        }
        let shelf = Shelf { y: self.next_y, h, x: w };
        self.next_y += h;
        let pos = (0, shelf.y);
        self.shelves.push(shelf);
        Some(pos)
    }

    fn mark_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let r = [x, y, x + w, y + h];
        self.dirty = Some(match self.dirty {
            None => r,
            Some(d) => [d[0].min(r[0]), d[1].min(r[1]), d[2].max(r[2]), d[3].max(r[3])],
        });
    }

    /// Забирает «грязную» область `[x0, y0, x1, y1]` для загрузки на GPU.
    ///
    /// Возвращает `None`, если атлас не менялся с прошлого вызова.
    pub fn take_dirty(&mut self) -> Option<[u32; 4]> {
        self.dirty.take()
    }

    /// Покрытие пикселя (для программного растеризатора).
    #[inline]
    pub fn sample(&self, x: u32, y: u32) -> u8 {
        self.pixels[(y * ATLAS_SIZE + x) as usize]
    }
}

/// Метрики начертания в пикселях для конкретного кегля.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    /// Подъём над базовой линией.
    pub ascent: f32,
    /// Спуск под базовую линию (положительное число).
    pub descent: f32,
}

/// Система шрифтов: начертания, кэш глифов, атлас.
pub struct Fonts {
    faces: Vec<FontRef<'static>>,
    upem: [f32; 6],
    cache: HashMap<GlyphKey, Option<GlyphEntry>>,
    chars: HashMap<(u8, char), (GlyphId, f32)>,
    /// Общий атлас покрытия.
    pub atlas: Atlas,
}

impl Fonts {
    /// Загружает встроенные шрифты и создаёт пустой атлас.
    ///
    /// ```
    /// let fonts = orbita_ui::gfx::text::Fonts::new();
    /// assert!(fonts.measure(&orbita_ui::tokens::text::BODY_MD, "Проводник", 1.0) > 40.0);
    /// ```
    pub fn new() -> Fonts {
        let mut faces = Vec::new();
        let mut upem = [1000.0; 6];
        for (i, face) in Face::ALL.iter().enumerate() {
            let data = if face.is_mono() { JETBRAINS_MONO_TTF } else { ONEST_TTF };
            let mut f = FontRef::try_from_slice(data).expect("встроенный шрифт повреждён");
            f.set_variation(b"wght", face.weight());
            upem[i] = f.units_per_em().unwrap_or(1000.0);
            faces.push(f);
        }
        Fonts { faces, upem, cache: HashMap::new(), chars: HashMap::new(), atlas: Atlas::new() }
    }

    fn font(&self, face: Face) -> &FontRef<'static> {
        &self.faces[face as usize]
    }

    /// Масштаб ab_glyph, соответствующий CSS-кеглю `size` (px на em).
    fn px_scale(&self, face: Face, size: f32) -> PxScale {
        let f = self.font(face);
        PxScale::from(size * f.height_unscaled() / self.upem[face as usize])
    }

    fn glyph_info(&mut self, face: Face, ch: char) -> (GlyphId, f32) {
        if let Some(v) = self.chars.get(&(face as u8, ch)) {
            return *v;
        }
        let f = &self.faces[face as usize];
        let mut id = f.glyph_id(ch);
        if id.0 == 0 && ch != ' ' {
            id = f.glyph_id('?');
        }
        let adv = f.h_advance_unscaled(id);
        self.chars.insert((face as u8, ch), (id, adv));
        (id, adv)
    }

    /// Метрики строки для стиля и масштаба.
    ///
    /// ```
    /// let fonts = orbita_ui::gfx::text::Fonts::new();
    /// let m = fonts.metrics(orbita_ui::gfx::text::Face::Sans400, 14.0);
    /// assert!(m.ascent > 10.0 && m.descent > 2.0);
    /// ```
    pub fn metrics(&self, face: Face, size: f32) -> LineMetrics {
        let f = self.font(face);
        let k = size / self.upem[face as usize];
        LineMetrics { ascent: f.ascent_unscaled() * k, descent: -f.descent_unscaled() * k }
    }

    /// Смещение базовой линии от верха строки высотой `style.line`
    /// (half-leading, как в CSS).
    ///
    /// Принимает: стиль текста и масштаб экрана.
    /// Возвращает: расстояние от верха строки до базовой линии (логические px).
    pub fn baseline(&self, style: &TextStyle) -> f32 {
        let m = self.metrics(style.face, style.size);
        (style.line - (m.ascent + m.descent)) * 0.5 + m.ascent
    }

    /// Ширина строки текста в логических пикселях.
    ///
    /// Принимает: `style` — стиль, `text` — строка (без переносов),
    /// `scale` — масштаб экрана (влияет только на хинтинг, не на результат).
    /// Возвращает: ширину, px.
    ///
    /// ```
    /// use orbita_ui::{gfx::text::Fonts, tokens::text};
    /// let mut fonts = Fonts::new();
    /// let a = fonts.measure(&text::BODY_MD, "abc", 1.0);
    /// let b = fonts.measure(&text::BODY_MD, "abcabc", 1.0);
    /// assert!((b - 2.0 * a).abs() < 1.0);
    /// ```
    pub fn measure(&self, style: &TextStyle, text: &str, _scale: f32) -> f32 {
        let f = self.font(style.face);
        let k = style.size / self.upem[style.face as usize];
        let mut w = 0.0;
        let mut prev: Option<GlyphId> = None;
        let mut count = 0usize;
        for ch in text.chars() {
            let id = match self.chars.get(&(style.face as u8, ch)) {
                Some((id, adv)) => {
                    w += adv * k;
                    *id
                }
                None => {
                    let mut id = f.glyph_id(ch);
                    if id.0 == 0 && ch != ' ' {
                        id = f.glyph_id('?');
                    }
                    w += f.h_advance_unscaled(id) * k;
                    id
                }
            };
            if let Some(p) = prev {
                w += f.kern_unscaled(p, id) * k;
            }
            prev = Some(id);
            count += 1;
        }
        if count > 1 {
            w += style.tracking * (count - 1) as f32;
        }
        w
    }

    /// Смещения начала каждого символа (и конец строки) — для курсора ввода.
    ///
    /// Возвращает вектор длиной `chars + 1`: `x` до каждого символа и в конце.
    ///
    /// ```
    /// use orbita_ui::{gfx::text::Fonts, tokens::text};
    /// let mut fonts = Fonts::new();
    /// let xs = fonts.caret_positions(&text::BODY_MD, "ab");
    /// assert_eq!(xs.len(), 3);
    /// assert_eq!(xs[0], 0.0);
    /// assert!(xs[2] > xs[1]);
    /// ```
    pub fn caret_positions(&mut self, style: &TextStyle, text: &str) -> Vec<f32> {
        let k = style.size / self.upem[style.face as usize];
        let mut out = Vec::with_capacity(text.len() + 1);
        let mut x = 0.0;
        let mut prev: Option<GlyphId> = None;
        out.push(0.0);
        for ch in text.chars() {
            let (id, adv) = self.glyph_info(style.face, ch);
            if let Some(p) = prev {
                x += self.font(style.face).kern_unscaled(p, id) * k;
            }
            x += adv * k + style.tracking;
            out.push(x);
            prev = Some(id);
        }
        out
    }

    /// Разбивает текст на строки не шире `max_w` по словам.
    ///
    /// Принимает: стиль, текст (может содержать `\n`), максимальную ширину.
    /// Возвращает: диапазоны байтов строк в исходном тексте.
    ///
    /// ```
    /// use orbita_ui::{gfx::text::Fonts, tokens::text};
    /// let mut fonts = Fonts::new();
    /// let t = "один два три четыре пять шесть";
    /// let lines = fonts.wrap(&text::BODY_MD, t, 80.0);
    /// assert!(lines.len() > 1);
    /// ```
    pub fn wrap(&mut self, style: &TextStyle, text: &str, max_w: f32) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (para_start, para) in split_inclusive_lines(text) {
            if para.is_empty() {
                out.push((para_start, para_start));
                continue;
            }
            let mut line_start = 0usize;
            let mut last_break: Option<usize> = None;
            let mut idx = 0usize;
            while idx < para.len() {
                let ch = para[idx..].chars().next().unwrap();
                let next = idx + ch.len_utf8();
                if ch == ' ' {
                    last_break = Some(idx);
                }
                let w = self.measure(style, &para[line_start..next], 1.0);
                if w > max_w && next - line_start > 1 {
                    let cut = match last_break {
                        Some(b) if b > line_start => b,
                        _ => idx,
                    };
                    out.push((para_start + line_start, para_start + cut));
                    line_start = if para[cut..].starts_with(' ') { cut + 1 } else { cut };
                    last_break = None;
                    idx = line_start;
                    continue;
                }
                idx = next;
            }
            out.push((para_start + line_start, para_start + para.len()));
        }
        out
    }

    /// Раскладывает строку в квадраты глифов (растеризуя новые в атлас).
    ///
    /// Принимает: `style`, `text`, позицию пера `(x, y_baseline)` в физических
    /// px, масштаб `scale` и колбэк, получающий каждый [`GlyphQuad`].
    /// Возвращает: продвижение пера (физические px).
    pub fn layout(
        &mut self,
        style: &TextStyle,
        text: &str,
        x: f32,
        baseline: f32,
        scale: f32,
        mut emit: impl FnMut(GlyphQuad),
    ) -> f32 {
        let size = style.size * scale;
        let size_q = math::round(size * 4.0) as u16;
        let k = size / self.upem[style.face as usize];
        let tracking = style.tracking * scale;
        let base_y = math::round(baseline);
        let mut pen = x;
        let mut prev: Option<GlyphId> = None;
        for ch in text.chars() {
            let (id, adv) = self.glyph_info(style.face, ch);
            if let Some(p) = prev {
                pen += self.font(style.face).kern_unscaled(p, id) * k;
            }
            prev = Some(id);
            if ch != ' ' && ch != '\t' {
                let fx = math::floor(pen);
                let sub = math::round((pen - fx) * 4.0) as u8 % 4;
                let fx = if math::round((pen - fx) * 4.0) as u8 == 4 { fx + 1.0 } else { fx };
                let key = GlyphKey { face: style.face as u8, size_q, glyph: id.0, sub };
                let entry = match self.cache.get(&key) {
                    Some(e) => *e,
                    None => {
                        let e = self.rasterize(style.face, id, size, sub);
                        self.cache.insert(key, e);
                        e
                    }
                };
                if let Some(e) = entry {
                    let inv = 1.0 / ATLAS_SIZE as f32;
                    emit(GlyphQuad {
                        rect: [fx + e.ox, base_y + e.oy, e.w as f32, e.h as f32],
                        uv: [
                            e.ax as f32 * inv,
                            e.ay as f32 * inv,
                            (e.ax + e.w) as f32 * inv,
                            (e.ay + e.h) as f32 * inv,
                        ],
                    });
                }
            }
            pen += adv * k + tracking;
        }
        pen - x
    }

    fn rasterize(&mut self, face: Face, id: GlyphId, size: f32, sub: u8) -> Option<GlyphEntry> {
        let scale = self.px_scale(face, size);
        let glyph = id.with_scale_and_position(scale, point(sub as f32 * 0.25, 0.0));
        let outlined = self.faces[face as usize].outline_glyph(glyph)?;
        let b = outlined.px_bounds();
        let w = (b.max.x - b.min.x) as u32;
        let h = (b.max.y - b.min.y) as u32;
        if w == 0 || h == 0 {
            return None;
        }
        let (ax, ay) = match self.atlas.alloc(w, h) {
            Some(p) => p,
            None => {
                // Атлас переполнен: сбрасываем кэш и начинаем заново.
                self.atlas.clear();
                self.cache.clear();
                self.atlas.alloc(w, h)?
            }
        };
        let pixels = &mut self.atlas.pixels;
        outlined.draw(|gx, gy, c| {
            let px = ax + gx;
            let py = ay + gy;
            if px < ATLAS_SIZE && py < ATLAS_SIZE {
                let v = (math::clamp(c, 0.0, 1.0) * 255.0 + 0.5) as u8;
                pixels[(py * ATLAS_SIZE + px) as usize] = v;
            }
        });
        self.atlas.mark_dirty(ax, ay, w, h);
        Some(GlyphEntry {
            ax: ax as u16,
            ay: ay as u16,
            w: w as u16,
            h: h as u16,
            ox: b.min.x,
            oy: b.min.y,
        })
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::new()
    }
}

/// Делит текст на абзацы по `\n`: (смещение начала, текст без `\n`).
fn split_inclusive_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            out.push((start, &text[start..i]));
            start = i + 1;
        }
    }
    out.push((start, &text[start..]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::text;

    #[test]
    fn cyrillic_glyphs_rasterize() {
        let mut fonts = Fonts::new();
        let mut n = 0;
        fonts.layout(&text::BODY_MD, "Привет", 0.0, 20.0, 1.0, |_| n += 1);
        assert_eq!(n, 6);
        assert!(fonts.atlas.take_dirty().is_some());
        assert!(fonts.atlas.take_dirty().is_none());
    }

    #[test]
    fn mono_is_monospaced() {
        let fonts = Fonts::new();
        let a = fonts.measure(&text::MONO_MD, "iiii", 1.0);
        let b = fonts.measure(&text::MONO_MD, "WWWW", 1.0);
        assert!((a - b).abs() < 0.01);
    }

    #[test]
    fn weights_differ() {
        let fonts = Fonts::new();
        let regular = fonts.measure(&text::BODY_MD, "Настройки", 1.0);
        let bold = fonts.measure(&text::BODY_MD.with_face(Face::Sans700), "Настройки", 1.0);
        assert!(bold > regular, "вариативная ось wght должна работать");
    }

    #[test]
    fn wrap_respects_newlines() {
        let mut fonts = Fonts::new();
        let lines = fonts.wrap(&text::BODY_MD, "a\nb\n\nc", 1000.0);
        assert_eq!(lines.len(), 4);
    }
}
