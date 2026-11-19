//! Векторные иконки дизайн-системы.
//!
//! Геометрия сгенерирована из SVG-символов макета скриптом
//! `scripts/gen_icons.py` в [`crate::icons_gen`]. Здесь — типы описания и
//! разрешение цветов слоёв по текущей теме.
//!
//! Правила из README дизайн-системы: значки линейные, сетка 24px, штрих
//! 1.75px, круглые концы, цвет `currentColor`; размеры 14/18/22px.

pub use crate::icons_gen::{ICON_DEFS, ICON_POINTS, Icon};

use crate::color::Color;
use crate::tokens::Theme;

/// Токен темы, на который ссылается слой иконки (`var(--…)` в SVG).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    /// `--cursor-fill`.
    CursorFill,
    /// `--cursor-outline`.
    CursorOutline,
    /// `--cursor-accent-fill`.
    CursorAccent,
    /// `--amb-cyan`.
    AmbCyan,
    /// `--amb-blue`.
    AmbBlue,
    /// `--amb-teal`.
    AmbTeal,
    /// `--amb-indigo`.
    AmbIndigo,
    /// `--accent-deep`.
    AccentDeep,
    /// `--accent`.
    Accent,
}

/// Цвет слоя иконки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconColor {
    /// `currentColor` — цвет, переданный при отрисовке.
    Current,
    /// Фиксированный `0xRRGGBBAA`.
    Rgba(u32),
    /// Токен темы.
    Token(Tok),
}

/// Заливка слоя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconPaint {
    /// Сплошной цвет.
    Solid(IconColor),
    /// Линейный градиент по диагонали габарита слоя (0,0 → 1,1).
    Linear(IconColor, IconColor),
    /// Радиальный градиент (центр 35%/30%, радиус 80%, три точки).
    Radial(IconColor, IconColor, IconColor),
}

/// Режим слоя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerMode {
    /// Заливка (правило чётности).
    Fill,
    /// Обводка с круглыми концами и стыками.
    Stroke,
}

/// Слой иконки: набор контуров с одним режимом и заливкой.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconLayer {
    /// Заливка или обводка.
    pub mode: LayerMode,
    /// Цвет/градиент.
    pub paint: IconPaint,
    /// Непрозрачность слоя.
    pub opacity: f32,
    /// Толщина обводки в единицах viewBox (для `Fill` — 0).
    pub width: f32,
    /// Контуры: диапазоны индексов в [`ICON_POINTS`].
    pub contours: &'static [(usize, usize)],
}

/// Описание иконки.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconDef {
    /// Имя символа в дизайн-системе (`i-folder`, `fi-doc`, `c-arrow`, `logo`).
    pub name: &'static str,
    /// Размер viewBox (квадрат): 24, 32 или 48.
    pub view: f32,
    /// Слои в порядке отрисовки.
    pub layers: &'static [IconLayer],
}

impl IconColor {
    /// Разрешает цвет по теме.
    ///
    /// Принимает: `theme` — текущая тема, `current` — цвет для `currentColor`.
    /// Возвращает: конкретный [`Color`].
    ///
    /// ```
    /// use orbita_ui::icons::{IconColor, Tok};
    /// use orbita_ui::{tokens::DARK, Color};
    /// assert_eq!(IconColor::Token(Tok::AmbBlue).resolve(&DARK, Color::WHITE), DARK.amb_blue);
    /// assert_eq!(IconColor::Current.resolve(&DARK, Color::BLACK), Color::BLACK);
    /// ```
    pub fn resolve(self, theme: &Theme, current: Color) -> Color {
        match self {
            IconColor::Current => current,
            IconColor::Rgba(v) => Color::hex(v),
            IconColor::Token(t) => match t {
                Tok::CursorFill => theme.cursor_fill,
                Tok::CursorOutline => theme.cursor_outline,
                Tok::CursorAccent => theme.cursor_accent_fill,
                Tok::AmbCyan => theme.amb_cyan,
                Tok::AmbBlue => theme.amb_blue,
                Tok::AmbTeal => theme.amb_teal,
                Tok::AmbIndigo => theme.amb_indigo,
                Tok::AccentDeep => theme.accent_deep,
                Tok::Accent => theme.accent,
            },
        }
    }
}

impl Icon {
    /// Ищет иконку по имени символа дизайн-системы.
    ///
    /// ```
    /// use orbita_ui::icons::Icon;
    /// assert_eq!(Icon::by_name("i-folder"), Some(Icon::IFolder));
    /// assert_eq!(Icon::by_name("nope"), None);
    /// ```
    pub fn by_name(name: &str) -> Option<Icon> {
        Icon::ALL.iter().copied().find(|i| i.def().name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_icons_have_geometry() {
        for icon in Icon::ALL {
            let def = icon.def();
            assert!(!def.layers.is_empty(), "{} has no layers", def.name);
            for layer in def.layers {
                for &(a, b) in layer.contours {
                    assert!(b > a && b <= ICON_POINTS.len());
                }
            }
        }
    }

    #[test]
    fn ui_icons_use_current_color_strokes() {
        let def = Icon::IFolder.def();
        assert_eq!(def.view, 24.0);
        assert_eq!(def.layers[0].mode, LayerMode::Stroke);
        assert_eq!(def.layers[0].paint, IconPaint::Solid(IconColor::Current));
        assert!((def.layers[0].width - 1.75).abs() < 1e-3);
    }
}
