//! Токены дизайн-системы Orbita OS (перенос `tokens.css` / `tokens.json`).
//!
//! Две темы — тёмная (`surface-base` #1f1f1f) и светлая (чистый белый).
//! Все компоненты берут цвета, отступы, радиусы, блюр, шум, типографику и
//! длительности анимаций только отсюда — как требует README дизайн-системы.

use crate::color::Color;
use crate::gfx::text::Face;

/// Режим темы.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    /// Тёмная тема (`data-theme="dark"`).
    Dark,
    /// Светлая тема (`data-theme="light"`).
    Light,
}

/// Полный набор цветовых токенов одной темы.
///
/// Имена полей совпадают с CSS-переменными (`--surface-base` → `surface_base`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct Theme {
    pub mode: ThemeMode,
    pub surface_base: Color,
    pub surface_raised: Color,
    pub surface_sunken: Color,
    pub surface_overlay: Color,
    pub glass_fill: Color,
    pub glass_fill_strong: Color,
    pub glass_fill_accent: Color,
    pub glass_border: Color,
    pub glass_highlight: Color,
    pub stroke_subtle: Color,
    pub stroke_default: Color,
    pub stroke_strong: Color,
    pub ink: Color,
    pub ink_muted: Color,
    pub ink_faint: Color,
    pub on_accent: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub accent_deep: Color,
    pub accent_text: Color,
    pub accent_soft: Color,
    pub accent_line: Color,
    pub focus_ring: Color,
    pub selection_bg: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub amb_blue: Color,
    pub amb_cyan: Color,
    pub amb_teal: Color,
    pub amb_indigo: Color,
    pub amb_rose: Color,
    pub amb_amber: Color,
    pub term_bg: Color,
    pub term_fg: Color,
    pub term_dim: Color,
    pub term_red: Color,
    pub term_green: Color,
    pub term_yellow: Color,
    pub term_blue: Color,
    pub term_magenta: Color,
    pub term_cyan: Color,
    pub term_selection: Color,
    pub term_cursor: Color,
    pub cursor_fill: Color,
    pub cursor_outline: Color,
    pub cursor_accent_fill: Color,
    /// Цвет теней (`#000` в тёмной, синеватый `#1e3a8a` в светлой).
    pub shadow: Color,
    /// Сила ambient-градиентов (`--amb-strength`: 72% / 46%).
    pub amb_strength: f32,
}

const fn c(v: u32) -> Color {
    Color::hex(v)
}

const fn rgb(v: u32) -> Color {
    Color::rgb_hex(v)
}

/// Тёмная тема (значения из `tokens.css`, блок `[data-theme="dark"]`).
pub const DARK: Theme = Theme {
    mode: ThemeMode::Dark,
    surface_base: rgb(0x1f1f1f),
    surface_raised: rgb(0x272727),
    surface_sunken: rgb(0x171717),
    surface_overlay: rgb(0x2c2c2e),
    glass_fill: c(0xffffff0f),
    glass_fill_strong: c(0xffffff1f),
    glass_fill_accent: c(0x2f5fe838),
    glass_border: c(0xffffff2e),
    glass_highlight: c(0xffffff40),
    stroke_subtle: c(0xffffff14),
    stroke_default: c(0xffffff29),
    stroke_strong: c(0xffffff66),
    ink: rgb(0xf2f3f5),
    ink_muted: rgb(0xb0b4bb),
    ink_faint: rgb(0x8f949c),
    on_accent: rgb(0xffffff),
    accent: rgb(0x2f5fe8),
    accent_hover: rgb(0x2754d1),
    accent_pressed: rgb(0x1f45b8),
    accent_deep: rgb(0x0b1f6e),
    accent_text: rgb(0x7da2ff),
    accent_soft: c(0x2f5fe840),
    accent_line: rgb(0x5b86ff),
    focus_ring: rgb(0x8fb0ff),
    selection_bg: c(0x2f5fe86b),
    success: rgb(0x4cc38a),
    warning: rgb(0xf0a43a),
    danger: rgb(0xff7a6e),
    amb_blue: rgb(0x1f5bff),
    amb_cyan: rgb(0x19b9e8),
    amb_teal: rgb(0x12b5a6),
    amb_indigo: rgb(0x3a35c9),
    amb_rose: rgb(0xe0508a),
    amb_amber: rgb(0xf2a65a),
    term_bg: rgb(0x121212),
    term_fg: rgb(0xe6e8eb),
    term_dim: rgb(0x8d929a),
    term_red: rgb(0xff7b72),
    term_green: rgb(0x5fd38d),
    term_yellow: rgb(0xf2c75c),
    term_blue: rgb(0x7aa2ff),
    term_magenta: rgb(0xd98cf5),
    term_cyan: rgb(0x4fd1e0),
    term_selection: c(0x2f5fe866),
    term_cursor: rgb(0x7da2ff),
    cursor_fill: rgb(0xffffff),
    cursor_outline: rgb(0x0b0b0c),
    cursor_accent_fill: rgb(0x2f5fe8),
    shadow: rgb(0x000000),
    amb_strength: 0.72,
};

/// Светлая тема (значения из `tokens.css`, блок `[data-theme="light"]`).
pub const LIGHT: Theme = Theme {
    mode: ThemeMode::Light,
    surface_base: rgb(0xffffff),
    surface_raised: rgb(0xf5f6f8),
    surface_sunken: rgb(0xeceef2),
    surface_overlay: rgb(0xffffff),
    glass_fill: c(0xffffff99),
    glass_fill_strong: c(0xffffffcc),
    glass_fill_accent: c(0x1646d61f),
    glass_border: c(0xffffffe6),
    glass_highlight: rgb(0xffffff),
    stroke_subtle: c(0x0f172a14),
    stroke_default: c(0x0f172a29),
    stroke_strong: c(0x0f172a80),
    ink: rgb(0x14161a),
    ink_muted: rgb(0x4d525b),
    ink_faint: rgb(0x656b75),
    on_accent: rgb(0xffffff),
    accent: rgb(0x1646d6),
    accent_hover: rgb(0x123bb4),
    accent_pressed: rgb(0x0e2f94),
    accent_deep: rgb(0x0a1f66),
    accent_text: rgb(0x1646d6),
    accent_soft: c(0x1646d61a),
    accent_line: rgb(0x1646d6),
    focus_ring: rgb(0x1646d6),
    selection_bg: c(0x1646d633),
    success: rgb(0x0b7a4f),
    warning: rgb(0x8a5300),
    danger: rgb(0xb3261e),
    amb_blue: rgb(0x2f6bff),
    amb_cyan: rgb(0x4cc9f0),
    amb_teal: rgb(0x2dd4bf),
    amb_indigo: rgb(0x5b5ff0),
    amb_rose: rgb(0xf472b6),
    amb_amber: rgb(0xfbbf77),
    term_bg: rgb(0xf7f8fa),
    term_fg: rgb(0x14161a),
    term_dim: rgb(0x636873),
    term_red: rgb(0xc02a2a),
    term_green: rgb(0x137a4a),
    term_yellow: rgb(0x8a5f00),
    term_blue: rgb(0x1646d6),
    term_magenta: rgb(0x8a2fb0),
    term_cyan: rgb(0x0b7285),
    term_selection: c(0x1646d62e),
    term_cursor: rgb(0x1646d6),
    cursor_fill: rgb(0x14161a),
    cursor_outline: rgb(0xffffff),
    cursor_accent_fill: rgb(0x1646d6),
    shadow: rgb(0x1e3a8a),
    amb_strength: 0.46,
};

/// Варианты акцентного цвета из настроек «Оформление» (свотчи на макете).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccentChoice {
    /// Глубокий синий — по умолчанию.
    Blue,
    /// Голубой.
    Cyan,
    /// Бирюзовый.
    Teal,
    /// Индиго.
    Indigo,
    /// Розовый.
    Rose,
}

impl AccentChoice {
    /// Все варианты в порядке свотчей.
    pub const ALL: [AccentChoice; 5] = [
        AccentChoice::Blue,
        AccentChoice::Cyan,
        AccentChoice::Teal,
        AccentChoice::Indigo,
        AccentChoice::Rose,
    ];

    /// Индекс варианта (для сохранения сессии).
    pub fn index(self) -> usize {
        AccentChoice::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }

    /// Вариант по индексу (неизвестный — синий).
    pub fn from_index(i: usize) -> AccentChoice {
        *AccentChoice::ALL.get(i).unwrap_or(&AccentChoice::Blue)
    }

    /// Базовый цвет свотча для темы.
    pub fn swatch(self, theme: &Theme) -> Color {
        match self {
            AccentChoice::Blue => theme.accent,
            AccentChoice::Cyan => theme.amb_cyan,
            AccentChoice::Teal => theme.amb_teal,
            AccentChoice::Indigo => theme.amb_indigo,
            AccentChoice::Rose => theme.amb_rose,
        }
    }
}

impl Theme {
    /// Тема по режиму.
    ///
    /// ```
    /// use orbita_ui::tokens::{Theme, ThemeMode};
    /// assert_eq!(Theme::for_mode(ThemeMode::Dark).surface_base, orbita_ui::Color::rgb_hex(0x1f1f1f));
    /// ```
    pub fn for_mode(mode: ThemeMode) -> Theme {
        match mode {
            ThemeMode::Dark => DARK,
            ThemeMode::Light => LIGHT,
        }
    }

    /// Пересчитывает семейство акцентных токенов под выбранный свотч.
    ///
    /// Синий оставляет значения дизайн-системы без изменений; остальные
    /// варианты выводят hover/pressed/text/soft/line/focus по тем же
    /// пропорциям, что и в исходной палитре.
    ///
    /// ```
    /// use orbita_ui::tokens::{AccentChoice, DARK};
    /// let t = DARK.with_accent(AccentChoice::Teal);
    /// assert_eq!(t.accent, DARK.amb_teal);
    /// assert_eq!(DARK.with_accent(AccentChoice::Blue), DARK);
    /// ```
    pub fn with_accent(self, choice: AccentChoice) -> Theme {
        if choice == AccentChoice::Blue {
            return self;
        }
        let base = choice.swatch(&self);
        let mut t = self;
        t.accent = base;
        t.accent_hover = base.mix(Color::BLACK, 0.11);
        t.accent_pressed = base.mix(Color::BLACK, 0.25);
        t.cursor_accent_fill = base;
        match self.mode {
            ThemeMode::Dark => {
                t.accent_text = base.mix(Color::WHITE, 0.45);
                t.accent_line = base.mix(Color::WHITE, 0.25);
                t.focus_ring = base.mix(Color::WHITE, 0.5);
                t.accent_soft = base.with_alpha(0.25);
                t.selection_bg = base.with_alpha(0.42);
                t.glass_fill_accent = base.with_alpha(0.22);
                t.term_cursor = t.accent_text;
                t.term_selection = base.with_alpha(0.4);
            }
            ThemeMode::Light => {
                let deep = base.mix(Color::BLACK, 0.2);
                t.accent = deep;
                t.accent_text = deep;
                t.accent_line = deep;
                t.focus_ring = deep;
                t.accent_soft = deep.with_alpha(0.1);
                t.selection_bg = deep.with_alpha(0.2);
                t.glass_fill_accent = deep.with_alpha(0.12);
                t.term_cursor = deep;
                t.term_selection = deep.with_alpha(0.18);
                t.cursor_accent_fill = deep;
            }
        }
        t
    }
}

/// Шкала отступов (шаг 4px): `space-1` … `space-16`.
pub mod space {
    /// 4px — зазор между иконкой и подписью.
    pub const S1: f32 = 4.0;
    /// 8px — внутренний зазор контролов.
    pub const S2: f32 = 8.0;
    /// 12px — горизонтальный padding кнопок и пунктов меню.
    pub const S3: f32 = 12.0;
    /// 16px — padding карточек и строк настроек.
    pub const S4: f32 = 16.0;
    /// 20px — padding окон и панелей.
    pub const S5: f32 = 20.0;
    /// 24px — зазор между блоками внутри окна.
    pub const S6: f32 = 24.0;
    /// 32px — отступ между секциями.
    pub const S8: f32 = 32.0;
    /// 40px — крупные отступы вокруг блоков внимания.
    pub const S10: f32 = 40.0;
    /// 48px — поля рабочего стола, высота дока.
    pub const S12: f32 = 48.0;
    /// 64px — максимальные отступы страниц.
    pub const S16: f32 = 64.0;
}

/// Радиусы скругления (`radius-xs` … `radius-pill`).
pub mod radius {
    /// 4px — метки, чекбоксы.
    pub const XS: f32 = 4.0;
    /// 8px — поля ввода, малые кнопки, пункты меню.
    pub const SM: f32 = 8.0;
    /// 12px — кнопки, плитки, дропдаун.
    pub const MD: f32 = 12.0;
    /// 16px — стеклянные панели, меню, карточки.
    pub const LG: f32 = 16.0;
    /// 24px — окна, блоки внимания.
    pub const XL: f32 = 24.0;
    /// 32px — док.
    pub const XXL: f32 = 32.0;
    /// Капсула (переключатели, теги).
    pub const PILL: f32 = 9999.0;
}

/// Блюр стекла (`blur-sm` … `blur-xl`), px стандартного отклонения.
pub mod blur {
    /// Тонкое стекло, тултипы.
    pub const SM: f32 = 8.0;
    /// Обычное стекло, кнопки, меню.
    pub const MD: f32 = 20.0;
    /// Окна и док.
    pub const LG: f32 = 40.0;
    /// Блоки внимания.
    pub const XL: f32 = 64.0;
}

/// Интенсивность шума (`noise-subtle` … `noise-strong`).
pub mod noise {
    /// Большие стеклянные поверхности, обои.
    pub const SUBTLE: f32 = 0.05;
    /// Панели и меню.
    pub const DEFAULT: f32 = 0.10;
    /// Блоки внимания.
    pub const STRONG: f32 = 0.18;
}

/// Длительности и кривые анимаций (`--ob-d-*`, `--ob-ease*`).
pub mod motion {
    /// 140ms — hover и переключатели.
    pub const FAST: f32 = 0.140;
    /// 240ms — меню, тени, перемещения.
    pub const BASE: f32 = 0.240;
    /// 520ms — док, крупные появления.
    pub const SLOW: f32 = 0.520;
    /// 420ms — появление окна (`ob-win`).
    pub const WINDOW: f32 = 0.420;
    /// 460ms — появление тоста (`ob-toast-in`).
    pub const TOAST: f32 = 0.460;
    /// `--ob-ease`: cubic-bezier(.22,.8,.24,1) — выразительное замедление.
    pub const EASE: [f32; 4] = [0.22, 0.8, 0.24, 1.0];
    /// `--ob-ease-soft`: cubic-bezier(.4,0,.2,1) — мягкий переход цвета.
    pub const EASE_SOFT: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
}

/// Стиль текста: начертание, кегль, интерлиньяж, трекинг.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Шрифт и насыщенность.
    pub face: Face,
    /// Кегль, px.
    pub size: f32,
    /// Высота строки, px.
    pub line: f32,
    /// Межбуквенный интервал, px.
    pub tracking: f32,
}

impl TextStyle {
    /// Тот же стиль другим начертанием.
    pub const fn with_face(self, face: Face) -> TextStyle {
        TextStyle { face, ..self }
    }
}

/// Типографическая шкала (`text-display-xl` … `text-mono-sm`).
pub mod text {
    use super::TextStyle;
    use crate::gfx::text::Face;

    /// 700 56/60, трекинг −1.4 — заставка.
    pub const DISPLAY_XL: TextStyle = TextStyle { face: Face::Sans700, size: 56.0, line: 60.0, tracking: -1.4 };
    /// 650 40/44 — заголовок экрана приветствия.
    pub const DISPLAY_LG: TextStyle = TextStyle { face: Face::Sans600, size: 40.0, line: 44.0, tracking: -0.8 };
    /// 600 24/30 — заголовок раздела.
    pub const TITLE_LG: TextStyle = TextStyle { face: Face::Sans600, size: 24.0, line: 30.0, tracking: -0.3 };
    /// 600 18/24 — заголовок карточки.
    pub const TITLE_MD: TextStyle = TextStyle { face: Face::Sans600, size: 18.0, line: 24.0, tracking: 0.0 };
    /// 600 15/20 — заголовок окна, группы настроек.
    pub const TITLE_SM: TextStyle = TextStyle { face: Face::Sans600, size: 15.0, line: 20.0, tracking: 0.0 };
    /// 400 16/24 — основной текст крупных экранов.
    pub const BODY_LG: TextStyle = TextStyle { face: Face::Sans400, size: 16.0, line: 24.0, tracking: 0.0 };
    /// 400 14/20 — основной текст интерфейса.
    pub const BODY_MD: TextStyle = TextStyle { face: Face::Sans400, size: 14.0, line: 20.0, tracking: 0.0 };
    /// 400 13/18 — вторичный текст.
    pub const BODY_SM: TextStyle = TextStyle { face: Face::Sans400, size: 13.0, line: 18.0, tracking: 0.0 };
    /// 500 14/20 — кнопки, меню, вкладки.
    pub const LABEL_MD: TextStyle = TextStyle { face: Face::Sans500, size: 14.0, line: 20.0, tracking: 0.0 };
    /// 500 13/18 — подписи панели, сегменты.
    pub const LABEL_13: TextStyle = TextStyle { face: Face::Sans500, size: 13.0, line: 18.0, tracking: 0.0 };
    /// 500 12/16 — малые кнопки, теги.
    pub const LABEL_SM: TextStyle = TextStyle { face: Face::Sans500, size: 12.0, line: 16.0, tracking: 0.1 };
    /// 600 11/14, капс — заголовки групп.
    pub const OVERLINE: TextStyle = TextStyle { face: Face::Sans600, size: 11.0, line: 14.0, tracking: 0.8 };
    /// 400 14/22 — терминал, код.
    pub const MONO_MD: TextStyle = TextStyle { face: Face::Mono400, size: 14.0, line: 22.0, tracking: 0.0 };
    /// 400 12/18 — пути, хеши, статусбар.
    pub const MONO_SM: TextStyle = TextStyle { face: Face::Mono400, size: 12.0, line: 18.0, tracking: 0.0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_have_distinct_bases() {
        assert_ne!(DARK.surface_base, LIGHT.surface_base);
        assert_eq!(LIGHT.surface_base, Color::WHITE);
    }

    #[test]
    fn accent_choice_roundtrip() {
        for a in AccentChoice::ALL {
            assert_eq!(AccentChoice::from_index(a.index()), a);
        }
    }

    #[test]
    fn custom_accent_changes_family() {
        let t = LIGHT.with_accent(AccentChoice::Rose);
        assert_ne!(t.accent, LIGHT.accent);
        assert_eq!(t.accent_line, t.accent);
    }
}
