//! # Orbita UI — дизайн-система и оболочка Orbita OS
//!
//! Крейт реализует дизайн-систему «Orbita OS» целиком: токены двух тем,
//! стекло с размытием и шумом, ambient-градиенты, типографику Onest /
//! JetBrains Mono, векторные иконки и курсоры, контролы, меню, окна, док,
//! верхнюю панель, лаунчер, уведомления — и базовые приложения
//! (терминал, проводник, настройки, монитор, редактор).
//!
//! Крейт `no_std + alloc`: один и тот же код работает
//!
//! * в ядре Orbita OS (UEFI, программный растеризатор [`gfx::soft`]);
//! * в эмуляторе на хосте (`emu/`), где кадр рисует настоящая видеокарта
//!   через wgpu, а оболочка перезагружается на лету (hot reload).
//!
//! ## Поток кадра
//!
//! ```text
//!  ввод (мышь/клавиатура) ──► Desktop::frame(&FrameInput)
//!                               │  оболочка + приложения (immediate mode)
//!                               ▼
//!                          Painter ──► Frame { instances, batches, segments }
//!                                                 │
//!                         GPU (wgpu)  ◄───────────┴──────────►  SoftRenderer
//! ```
//!
//! ## Пример
//!
//! ```
//! use orbita_ui::{Desktop, FrameInput};
//! let mut desktop = Desktop::new(orbita_ui::system::DemoSystem::boxed());
//! let input = FrameInput { width: 1280, height: 720, scale: 1.0, time: 0.0, ..Default::default() };
//! let frame = desktop.frame(&input);
//! assert!(frame.instances.len() > 50);
//! ```

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

pub mod abi;
pub mod anim;
pub mod apps;
pub mod color;
pub mod geom;
pub mod gfx;
pub mod icons;
#[allow(missing_docs)]
mod icons_gen;
pub mod input;
pub mod math;
pub mod session;
pub mod shell;
pub mod system;
pub mod tokens;
pub mod ui;

pub use color::Color;
pub use geom::{Radii, Rect, Vec2};
pub use input::{FrameInput, Key, KeyEvent, Modifiers, MouseButtons};
pub use shell::Desktop;
