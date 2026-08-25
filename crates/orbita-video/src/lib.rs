#![no_std]

//! Video and UI primitives for framebuffer-based rendering.
//!
//! The crate stays `no_std` so it can be used in early boot and kernel code
//! without depending on allocation or a host runtime.

extern crate alloc;

mod backend;
mod color;
pub mod edid;
mod font;
mod framebuffer;
mod gui;
mod image;
mod primitives;
mod screen;
mod surface;
mod text;

pub use color::{Color, PixelFormat};
pub use backend::{
    BackendInfo, FrameCompositor, PresentBackend, RendererDiagnostics, SOFTWARE_FRAMEBUFFER,
    SoftwareFramebuffer, backend_info, backend_names, create_backend, register_backend,
};
pub use edid::{EdidInfo, EdidTiming};
pub use font::{BitmapFont, BuiltinFont, GlyphBitmap, MemoryFont, MemoryGlyph};
pub use framebuffer::{Framebuffer, FramebufferInfo};
pub use gui::{BackBuffer, CursorKind, CursorStyle, DesktopTheme, GuiCanvas, inside_rounded_rect};
pub use image::{ImageView, OwnedImage};
pub use primitives::{Insets, Point, Rect, Size};
pub use screen::{Connector, DisplayInfo, ModeInfo, MonitorInventory};
pub use surface::Surface;
pub use text::{TextAlign, TextMetrics, TextRenderer, TextStyle, TextWrap};
