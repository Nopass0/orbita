#![no_std]

extern crate alloc;

mod model;
mod render;

pub use model::{BootSplash, DesktopConsoleSnapshot, DesktopScene};
pub use render::{DesktopRenderer, RedrawScope};
