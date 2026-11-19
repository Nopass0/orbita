//! C-ABI горячей перезагрузки: граница между хостом эмулятора и DLL
//! оболочки.
//!
//! Хост (окно, GPU, таймер) и оболочка (весь интерфейс) собираются
//! отдельно. Хост грузит `orbita_emu_shell.dll`, вызывает её функции и при
//! изменении исходников подменяет DLL на новую, не закрывая окно. Чтобы
//! это было безопасно, через границу ходят только `#[repr(C)]`-структуры
//! из этого модуля и POD-массивы [`Instance`]/[`Batch`]. При несовместимом
//! изменении увеличивается [`ABI_VERSION`] — хост это заметит и попросит
//! перезапуск.
//!
//! ```text
//!  orbita-emu (хост)                 orbita_emu_shell.dll (оболочка)
//!  ────────────────                  ───────────────────────────────
//!  winit события ──► FrameInputC ──► orbita_shell_frame()
//!  wgpu рендер   ◄── FrameOutputC ◄── Desktop::frame() → Frame
//!  watcher: cargo build → save_session → выгрузить → загрузить → restore
//! ```

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::geom::Vec2;
use crate::gfx::text::{ATLAS_SIZE, Atlas};
use crate::gfx::{Batch, Frame, Instance};
use crate::input::{FrameInput, Key, KeyEvent, Modifiers, MouseButtons};

/// Версия ABI. Меняется при любом изменении структур этого модуля
/// или [`Instance`]/[`Batch`].
pub const ABI_VERSION: u32 = 3;

/// Событие клавиши в C-представлении.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct KeyEventC {
    /// Код клавиши (см. [`key_to_code`]).
    pub code: u32,
    /// 1 — нажатие, 0 — отпускание.
    pub down: u8,
    /// Модификаторы: бит 0 Shift, 1 Ctrl, 2 Alt, 3 Win.
    pub mods: u8,
}

/// Ввод кадра в C-представлении.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FrameInputC {
    /// Ширина цели, физические px.
    pub width: u32,
    /// Высота цели.
    pub height: u32,
    /// Масштаб HiDPI.
    pub scale: f32,
    /// Время кадра, сек.
    pub time: f64,
    /// Указатель X (логические px).
    pub mouse_x: f32,
    /// Указатель Y.
    pub mouse_y: f32,
    /// 1 — курсор внутри окна.
    pub mouse_in: u8,
    /// Кнопки мыши.
    pub buttons: u8,
    /// Модификаторы.
    pub mods: u8,
    /// Прокрутка X.
    pub wheel_x: f32,
    /// Прокрутка Y.
    pub wheel_y: f32,
    /// События клавиш.
    pub keys: *const KeyEventC,
    /// Число событий.
    pub keys_len: usize,
    /// Набранный текст (UTF-8).
    pub text: *const u8,
    /// Длина текста в байтах.
    pub text_len: usize,
    /// FPS, измеренный хостом.
    pub fps: f32,
    /// CPU-время прошлого кадра, мс.
    pub cpu_ms: f32,
    /// GPU-время прошлого кадра, мс.
    pub gpu_ms: f32,
}

/// Результат кадра в C-представлении (указатели живут до следующего вызова).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct FrameOutputC {
    /// Инстансы.
    pub instances: *const Instance,
    /// Их число.
    pub instances_len: usize,
    /// Пакеты.
    pub batches: *const Batch,
    /// Их число.
    pub batches_len: usize,
    /// Отрезки путей.
    pub segments: *const [f32; 4],
    /// Их число.
    pub segments_len: usize,
    /// Цвет очистки.
    pub clear: [f32; 4],
    /// Пиксели атласа глифов (R8, `atlas_size²`).
    pub atlas: *const u8,
    /// Сторона атласа.
    pub atlas_size: u32,
    /// Грязная область атласа `[x0, y0, x1, y1]`.
    pub atlas_dirty: [u32; 4],
    /// 1 — грязная область валидна.
    pub atlas_dirty_valid: u32,
    /// Поколение атласа (смена — перезалить целиком).
    pub atlas_generation: u32,
    /// Просьба включить vsync.
    pub vsync: u32,
}

const K_ENTER: u32 = 1;
const K_BACKSPACE: u32 = 2;
const K_DELETE: u32 = 3;
const K_TAB: u32 = 4;
const K_ESCAPE: u32 = 5;
const K_LEFT: u32 = 6;
const K_RIGHT: u32 = 7;
const K_UP: u32 = 8;
const K_DOWN: u32 = 9;
const K_HOME: u32 = 10;
const K_END: u32 = 11;
const K_PGUP: u32 = 12;
const K_PGDN: u32 = 13;
const K_SPACE: u32 = 14;
const K_SUPER: u32 = 15;
const K_F: u32 = 100;
const K_CHAR: u32 = 0x1000_0000;

/// Клавиша → код ABI.
///
/// ```
/// use orbita_ui::{Key, abi::{key_to_code, code_to_key}};
/// for k in [Key::Enter, Key::F(5), Key::Char('ж'), Key::Super] {
///     assert_eq!(code_to_key(key_to_code(k)), Some(k));
/// }
/// ```
pub fn key_to_code(k: Key) -> u32 {
    match k {
        Key::Enter => K_ENTER,
        Key::Backspace => K_BACKSPACE,
        Key::Delete => K_DELETE,
        Key::Tab => K_TAB,
        Key::Escape => K_ESCAPE,
        Key::Left => K_LEFT,
        Key::Right => K_RIGHT,
        Key::Up => K_UP,
        Key::Down => K_DOWN,
        Key::Home => K_HOME,
        Key::End => K_END,
        Key::PageUp => K_PGUP,
        Key::PageDown => K_PGDN,
        Key::Space => K_SPACE,
        Key::Super => K_SUPER,
        Key::F(n) => K_F + n as u32,
        Key::Char(c) => K_CHAR | c as u32,
    }
}

/// Код ABI → клавиша.
pub fn code_to_key(c: u32) -> Option<Key> {
    Some(match c {
        K_ENTER => Key::Enter,
        K_BACKSPACE => Key::Backspace,
        K_DELETE => Key::Delete,
        K_TAB => Key::Tab,
        K_ESCAPE => Key::Escape,
        K_LEFT => Key::Left,
        K_RIGHT => Key::Right,
        K_UP => Key::Up,
        K_DOWN => Key::Down,
        K_HOME => Key::Home,
        K_END => Key::End,
        K_PGUP => Key::PageUp,
        K_PGDN => Key::PageDown,
        K_SPACE => Key::Space,
        K_SUPER => Key::Super,
        c if (K_F..K_F + 25).contains(&c) => Key::F((c - K_F) as u8),
        c if c & K_CHAR != 0 => Key::Char(char::from_u32(c & !K_CHAR)?),
        _ => return None,
    })
}

/// Модификаторы → битовая маска.
pub fn mods_to_bits(m: Modifiers) -> u8 {
    (m.shift as u8) | ((m.ctrl as u8) << 1) | ((m.alt as u8) << 2) | ((m.logo as u8) << 3)
}

/// Битовая маска → модификаторы.
pub fn bits_to_mods(b: u8) -> Modifiers {
    Modifiers { shift: b & 1 != 0, ctrl: b & 2 != 0, alt: b & 4 != 0, logo: b & 8 != 0 }
}

impl FrameInputC {
    /// Переводит в [`FrameInput`].
    ///
    /// # Safety
    /// Указатели `keys`/`text` должны быть валидны на длину `*_len`.
    pub unsafe fn to_input(&self) -> FrameInput {
        let keys = if self.keys.is_null() || self.keys_len == 0 {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(self.keys, self.keys_len) }
        };
        let text = if self.text.is_null() || self.text_len == 0 {
            String::new()
        } else {
            let bytes = unsafe { core::slice::from_raw_parts(self.text, self.text_len) };
            String::from_utf8_lossy(bytes).into_owned()
        };
        let keys: Vec<KeyEvent> = keys
            .iter()
            .filter_map(|k| code_to_key(k.code).map(|key| KeyEvent { key, down: k.down != 0, mods: bits_to_mods(k.mods) }))
            .collect();
        FrameInput {
            width: self.width,
            height: self.height,
            scale: self.scale,
            time: self.time,
            mouse: if self.mouse_in != 0 { Some(Vec2::new(self.mouse_x, self.mouse_y)) } else { None },
            buttons: MouseButtons(self.buttons),
            wheel: Vec2::new(self.wheel_x, self.wheel_y),
            keys,
            text,
            mods: bits_to_mods(self.mods),
            fps: self.fps,
            cpu_ms: self.cpu_ms,
            gpu_ms: self.gpu_ms,
        }
    }
}

impl FrameOutputC {
    /// Упаковывает кадр и атлас (забирая грязную область атласа).
    pub fn pack(frame: &Frame, atlas: &mut Atlas, vsync: bool) -> FrameOutputC {
        let dirty = atlas.take_dirty();
        FrameOutputC {
            instances: frame.instances.as_ptr(),
            instances_len: frame.instances.len(),
            batches: frame.batches.as_ptr(),
            batches_len: frame.batches.len(),
            segments: frame.segments.as_ptr(),
            segments_len: frame.segments.len(),
            clear: frame.clear,
            atlas: atlas.pixels.as_ptr(),
            atlas_size: ATLAS_SIZE,
            atlas_dirty: dirty.unwrap_or([0; 4]),
            atlas_dirty_valid: dirty.is_some() as u32,
            atlas_generation: atlas.generation,
            vsync: vsync as u32,
        }
    }
}
