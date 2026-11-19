//! Цвет в пространстве sRGB с прямой (непредумноженной) альфой.
//!
//! Дизайн-система задаёт цвета в hex (`#2f5fe8`, `#ffffff0f`) и смешивает
//! их через `color-mix(in srgb, …)`, а браузер композитит в гамма-пространстве.
//! Чтобы рендер совпадал с макетами пиксель-в-пиксель, все вычисления
//! ведутся прямо в sRGB-значениях (без перевода в линейное пространство).

use crate::math;

/// Цвет RGBA, компоненты в диапазоне `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    /// Красный канал (sRGB).
    pub r: f32,
    /// Зелёный канал (sRGB).
    pub g: f32,
    /// Синий канал (sRGB).
    pub b: f32,
    /// Непрозрачность: 0 — прозрачный, 1 — сплошной.
    pub a: f32,
}

impl Color {
    /// Полностью прозрачный цвет.
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    /// Белый.
    pub const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
    /// Чёрный.
    pub const BLACK: Color = Color::rgba(0.0, 0.0, 0.0, 1.0);

    /// Создаёт цвет из компонент `0.0..=1.0`.
    ///
    /// ```
    /// let c = orbita_ui::Color::rgba(1.0, 0.5, 0.0, 1.0);
    /// assert_eq!(c.g, 0.5);
    /// ```
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    /// Создаёт цвет из 32-битного значения `0xRRGGBBAA` (как `#rrggbbaa` в CSS).
    ///
    /// Принимает: `v` — упакованный цвет, младший байт — альфа.
    /// Возвращает: [`Color`].
    ///
    /// ```
    /// let accent = orbita_ui::Color::hex(0x2f5fe8ff);
    /// assert!((accent.r - 47.0 / 255.0).abs() < 1e-6);
    /// assert_eq!(accent.a, 1.0);
    /// ```
    pub const fn hex(v: u32) -> Color {
        Color {
            r: ((v >> 24) & 0xff) as f32 / 255.0,
            g: ((v >> 16) & 0xff) as f32 / 255.0,
            b: ((v >> 8) & 0xff) as f32 / 255.0,
            a: (v & 0xff) as f32 / 255.0,
        }
    }

    /// Создаёт непрозрачный цвет из `0xRRGGBB`.
    ///
    /// ```
    /// assert_eq!(orbita_ui::Color::rgb_hex(0x1f1f1f).a, 1.0);
    /// ```
    pub const fn rgb_hex(v: u32) -> Color {
        Color::hex((v << 8) | 0xff)
    }

    /// Возвращает тот же цвет с другой непрозрачностью.
    ///
    /// ```
    /// let c = orbita_ui::Color::WHITE.with_alpha(0.25);
    /// assert_eq!(c.a, 0.25);
    /// ```
    pub const fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// Умножает непрозрачность на `k` (групповая прозрачность при анимациях).
    ///
    /// ```
    /// let c = orbita_ui::Color::WHITE.with_alpha(0.5).fade(0.5);
    /// assert_eq!(c.a, 0.25);
    /// ```
    pub fn fade(self, k: f32) -> Color {
        Color { a: self.a * k, ..self }
    }

    /// Смешивание как `color-mix(in srgb, self (1-t), other t)`.
    ///
    /// Принимает: `other` — второй цвет, `t` — доля второго цвета `0..=1`.
    /// Возвращает: смешанный цвет (альфа тоже интерполируется, с учётом
    /// предумножения, как в CSS Color 5).
    ///
    /// ```
    /// use orbita_ui::Color;
    /// let mid = Color::BLACK.mix(Color::WHITE, 0.5);
    /// assert!((mid.r - 0.5).abs() < 1e-6);
    /// ```
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = math::clamp(t, 0.0, 1.0);
        let a = math::lerp(self.a, other.a, t);
        if a <= 1e-6 {
            return Color::TRANSPARENT;
        }
        // Предумноженная интерполяция, затем обратно в прямую альфу.
        let r = (self.r * self.a * (1.0 - t) + other.r * other.a * t) / a;
        let g = (self.g * self.a * (1.0 - t) + other.g * other.a * t) / a;
        let b = (self.b * self.a * (1.0 - t) + other.b * other.a * t) / a;
        Color { r, g, b, a }
    }

    /// `color-mix(in srgb, self p%, transparent)` — цвет с долей `p`.
    ///
    /// ```
    /// let c = orbita_ui::Color::WHITE.percent(0.72);
    /// assert!((c.a - 0.72).abs() < 1e-6);
    /// ```
    pub fn percent(self, p: f32) -> Color {
        self.fade(math::clamp(p, 0.0, 1.0))
    }

    /// Упаковывает в массив `[r, g, b, a]` (формат GPU-инстансов).
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Обратное к [`Color::to_array`].
    pub const fn from_array(v: [f32; 4]) -> Color {
        Color { r: v[0], g: v[1], b: v[2], a: v[3] }
    }

    /// Относительная яркость (sRGB-веса Rec.709), для насыщенности стекла.
    ///
    /// ```
    /// assert!((orbita_ui::Color::WHITE.luma() - 1.0).abs() < 1e-4);
    /// ```
    pub fn luma(self) -> f32 {
        self.r * 0.2126 + self.g * 0.7152 + self.b * 0.0722
    }

    /// Изменяет насыщенность (как CSS `saturate(k)`): 1 — без изменений,
    /// 0 — серый, 1.8 — «сочнее» (так ведёт себя стекло Orbita).
    ///
    /// ```
    /// use orbita_ui::Color;
    /// let grey = Color::rgb_hex(0x2f5fe8).saturate(0.0);
    /// assert!((grey.r - grey.b).abs() < 1e-6);
    /// ```
    pub fn saturate(self, k: f32) -> Color {
        let l = self.luma();
        Color {
            r: math::clamp(l + (self.r - l) * k, 0.0, 1.0),
            g: math::clamp(l + (self.g - l) * k, 0.0, 1.0),
            b: math::clamp(l + (self.b - l) * k, 0.0, 1.0),
            a: self.a,
        }
    }

    /// Упаковывает в `0xAARRGGBB`-пиксель (формат UEFI GOP BGRA little-endian).
    ///
    /// ```
    /// assert_eq!(orbita_ui::Color::rgb_hex(0x102030).to_bgra_u32(), 0xff102030);
    /// ```
    pub fn to_bgra_u32(self) -> u32 {
        let r = (math::clamp(self.r, 0.0, 1.0) * 255.0 + 0.5) as u32;
        let g = (math::clamp(self.g, 0.0, 1.0) * 255.0 + 0.5) as u32;
        let b = (math::clamp(self.b, 0.0, 1.0) * 255.0 + 0.5) as u32;
        let a = (math::clamp(self.a, 0.0, 1.0) * 255.0 + 0.5) as u32;
        (a << 24) | (r << 16) | (g << 8) | b
    }
}
