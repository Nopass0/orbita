//! Ввод кадра: состояние мыши, клавиши и набранный текст.
//!
//! Платформа (ядро с PS/2 или эмулятор с winit) собирает события за кадр
//! в [`FrameInput`] и передаёт оболочке. Оболочка сама вычисляет нажатия,
//! отпускания, двойные клики и перетаскивания, сравнивая с прошлым кадром.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::geom::Vec2;

/// Кнопки мыши (битовая маска).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseButtons(pub u8);

impl MouseButtons {
    /// Левая кнопка.
    pub const LEFT: u8 = 1;
    /// Правая кнопка.
    pub const RIGHT: u8 = 2;
    /// Средняя кнопка.
    pub const MIDDLE: u8 = 4;

    /// Нажата ли кнопка `bit`.
    ///
    /// ```
    /// use orbita_ui::MouseButtons;
    /// assert!(MouseButtons(MouseButtons::LEFT).has(MouseButtons::LEFT));
    /// assert!(!MouseButtons(0).has(MouseButtons::RIGHT));
    /// ```
    pub fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

/// Модификаторы клавиатуры.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Ctrl.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
    /// Win / Super.
    pub logo: bool,
}

/// Клавиша (не символ — символы приходят в [`FrameInput::text`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Key {
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    /// Функциональная клавиша F1…F12.
    F(u8),
    /// Буква/цифра для сочетаний (`Ctrl+C` → `Char('c')`), всегда в нижнем регистре.
    Char(char),
    /// Клавиша Win/Super.
    Super,
}

/// Событие клавиатуры.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// Клавиша.
    pub key: Key,
    /// true — нажатие (и автоповтор), false — отпускание.
    pub down: bool,
    /// Модификаторы в момент события.
    pub mods: Modifiers,
}

/// Всё, что платформа сообщает оболочке за один кадр.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameInput {
    /// Ширина экрана/окна в физических пикселях.
    pub width: u32,
    /// Высота в физических пикселях.
    pub height: u32,
    /// Масштаб (физических px на логический), 1.0 — без HiDPI.
    pub scale: f32,
    /// Монотонное время кадра в секундах.
    pub time: f64,
    /// Положение указателя в логических px (`None` — курсор вне окна).
    pub mouse: Option<Vec2>,
    /// Зажатые кнопки мыши.
    pub buttons: MouseButtons,
    /// Прокрутка колесом за кадр (логические px, вниз — положительно).
    pub wheel: Vec2,
    /// События клавиш за кадр.
    pub keys: Vec<KeyEvent>,
    /// Набранный текст за кадр (уже с учётом раскладки и Shift).
    pub text: String,
    /// Текущие модификаторы.
    pub mods: Modifiers,
    /// Измеренная платформой частота кадров (для монитора и топбара).
    pub fps: f32,
    /// Время на CPU-подготовку прошлого кадра, мс.
    pub cpu_ms: f32,
    /// Время на GPU/растеризацию прошлого кадра, мс.
    pub gpu_ms: f32,
}

impl FrameInput {
    /// Была ли в кадре нажата клавиша `key`.
    ///
    /// ```
    /// use orbita_ui::{FrameInput, Key, KeyEvent, Modifiers};
    /// let mut i = FrameInput::default();
    /// i.keys.push(KeyEvent { key: Key::Enter, down: true, mods: Modifiers::default() });
    /// assert!(i.pressed(Key::Enter));
    /// assert!(!i.pressed(Key::Escape));
    /// ```
    pub fn pressed(&self, key: Key) -> bool {
        self.keys.iter().any(|k| k.down && k.key == key)
    }

    /// Было ли нажато сочетание с Ctrl.
    pub fn ctrl(&self, ch: char) -> bool {
        self.keys.iter().any(|k| k.down && k.mods.ctrl && k.key == Key::Char(ch))
    }
}
