//! Graphics backend contract and the default software-framebuffer engine.
//!
//! # Architecture
//!
//! All desktop drawing happens into CPU-side [`crate::BackBuffer`]s. The
//! *present seam* — how a finished back buffer reaches the visible scanout —
//! is the [`PresentBackend`] trait:
//!
//! - [`SoftwareFramebuffer`] (default): copies dirty rows into the linear
//!   firmware framebuffer (UEFI GOP) — exactly the historic path, with
//!   dirty-rect presentation.
//! - A **driver module** can supply any other engine (Vulkan, virtio-gpu,
//!   a DRM scanout, a network display, ...) by implementing
//!   [`PresentBackend`] and registering a factory with [`register_backend`].
//!   Consumers keep calling the same compositor API; nothing else changes.
//!
//! # Registering a custom engine (driver side)
//!
//! ```ignore
//! struct VulkanBackend { /* device, queues, swapchain ... */ }
//!
//! impl orbita_video::PresentBackend for VulkanBackend {
//!     fn info(&self) -> orbita_video::BackendInfo { /* ... */ }
//!     fn present_region(&mut self, pixels: &[Color], stride: usize, region: Rect) {
//!         /* upload `region` of the buffer and queue a present */
//!     }
//! }
//!
//! orbita_video::register_backend(
//!     BackendInfo { name: "vulkan", api: "vulkan-1.3", ..BackendInfo::software_default() },
//!     |fb| Box::new(VulkanBackend::new(fb)),
//! );
//! ```
//!
//! The kernel then selects the engine by name (e.g. `gfx=vulkan` in
//! `/etc/orbita.conf`); unknown names fall back to the software backend.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;

use crate::{Color, Framebuffer, FramebufferInfo, Rect, Size};

/// Canonical name of the built-in software framebuffer backend.
pub const SOFTWARE_FRAMEBUFFER: &str = "software-framebuffer";

/// Identity and capabilities of a present backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendInfo {
    /// Registry name, e.g. `"software-framebuffer"` or `"vulkan"`.
    pub name: &'static str,
    /// Rendering API behind the present path, e.g. `"cpu-raster"`.
    pub api: &'static str,
    /// Presentation style, e.g. `"double-buffered"`.
    pub present_mode: &'static str,
    /// How many back buffers the compositor keeps for this backend.
    pub swapchain_len: usize,
    /// Whether presentation is hardware-accelerated.
    pub accelerated: bool,
}

impl BackendInfo {
    /// Info of the built-in software framebuffer backend.
    pub const fn software() -> Self {
        Self {
            name: SOFTWARE_FRAMEBUFFER,
            api: "cpu-raster",
            present_mode: "double-buffered-dirty-rect",
            swapchain_len: 1,
            accelerated: false,
        }
    }
}

/// The present seam: publishes finished pixels to the scanout.
///
/// `pixels` is a full-surface buffer of `stride` pixels per row; only
/// `region` (already clipped to the surface) needs to be pushed.
pub trait PresentBackend {
    /// Identity and capabilities.
    fn info(&self) -> BackendInfo;
    /// Publish `region` of a completed back buffer to the scanout.
    fn present_region(&mut self, pixels: &[Color], stride: usize, region: Rect);
}

/// The default engine: writes dirty rows into the linear firmware
/// framebuffer. Works on any UEFI GOP scanout; no GPU driver required.
pub struct SoftwareFramebuffer {
    framebuffer: Framebuffer,
    info: BackendInfo,
}

impl SoftwareFramebuffer {
    /// Wrap a linear framebuffer described by `info`.
    pub fn new(info: FramebufferInfo) -> Self {
        Self {
            framebuffer: Framebuffer::new(info),
            info: BackendInfo::software(),
        }
    }
}

impl PresentBackend for SoftwareFramebuffer {
    fn info(&self) -> BackendInfo {
        self.info
    }

    fn present_region(&mut self, pixels: &[Color], stride: usize, region: Rect) {
        let width = stride.min(self.framebuffer.width());
        let height = (pixels.len() / stride.max(1)).min(self.framebuffer.height());
        let Some(clipped) = region.intersect(Rect::new(0, 0, width, height)) else {
            return;
        };
        for y in clipped.y..clipped.bottom() {
            let row = &pixels[y * stride + clipped.x..y * stride + clipped.right()];
            self.framebuffer.write_row(clipped.x, y, row);
        }
    }
}

type BackendFactory = fn(FramebufferInfo) -> Box<dyn PresentBackend>;

struct BackendDescriptor {
    info: BackendInfo,
    factory: BackendFactory,
}

static REGISTRY: Mutex<Vec<BackendDescriptor>> = Mutex::new(Vec::new());

fn software_factory(info: FramebufferInfo) -> Box<dyn PresentBackend> {
    Box::new(SoftwareFramebuffer::new(info))
}

fn ensure_defaults(registry: &mut Vec<BackendDescriptor>) {
    if !registry.iter().any(|d| d.info.name == SOFTWARE_FRAMEBUFFER) {
        registry.push(BackendDescriptor {
            info: BackendInfo::software(),
            factory: software_factory,
        });
    }
}

/// Register (or replace, by name) a backend factory. Driver modules call
/// this at init; the compositor resolves by name at (re)configuration.
pub fn register_backend(info: BackendInfo, factory: BackendFactory) {
    let mut registry = REGISTRY.lock();
    ensure_defaults(&mut registry);
    registry.retain(|d| d.info.name != info.name);
    registry.push(BackendDescriptor { info, factory });
}

/// Names of all registered backends (software first).
pub fn backend_names() -> Vec<&'static str> {
    let mut registry = REGISTRY.lock();
    ensure_defaults(&mut registry);
    registry.iter().map(|d| d.info.name).collect()
}

/// Look up backend info by name; falls back to the software backend.
pub fn backend_info(name: &str) -> BackendInfo {
    let mut registry = REGISTRY.lock();
    ensure_defaults(&mut registry);
    registry
        .iter()
        .find(|d| d.info.name == name)
        .map(|d| d.info)
        .unwrap_or_else(BackendInfo::software)
}

/// Instantiate the named backend around the given scanout; falls back to
/// the software backend when the name is unknown.
pub fn create_backend(name: &str, scanout: FramebufferInfo) -> Box<dyn PresentBackend> {
    let mut registry = REGISTRY.lock();
    ensure_defaults(&mut registry);
    let factory = registry
        .iter()
        .find(|d| d.info.name == name)
        .map(|d| d.factory)
        .unwrap_or(software_factory);
    factory(scanout)
}

/// Live diagnostics of a compositor's backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RendererDiagnostics {
    pub backend_name: &'static str,
    pub api: &'static str,
    pub present_mode: &'static str,
    pub frame_index: usize,
    pub frames_in_flight: usize,
}

/// Owns the back-buffer swapchain and the present backend.
///
/// Consumers draw through [`FrameCompositor::canvas`] and publish with
/// [`FrameCompositor::present_region`] — the backend decides where the
/// pixels actually go.
pub struct FrameCompositor {
    backend: Box<dyn PresentBackend>,
    swapchain: Vec<crate::BackBuffer>,
    acquire_index: usize,
    present_count: u64,
}

impl FrameCompositor {
    /// Build a compositor over `backend` with a swapchain sized to the
    /// backend's advertised depth.
    pub fn new(size: Size, backend: Box<dyn PresentBackend>) -> Self {
        let swapchain_len = backend.info().swapchain_len.max(1);
        let mut swapchain = Vec::new();
        for _ in 0..swapchain_len {
            swapchain.push(crate::BackBuffer::new(size));
        }
        Self {
            backend,
            swapchain,
            acquire_index: 0,
            present_count: 0,
        }
    }

    /// Info of the active backend.
    pub fn backend_info(&self) -> BackendInfo {
        self.backend.info()
    }

    /// Reconfigure with a new size and/or backend, reallocating only when
    /// something actually changed.
    pub fn reconfigure(&mut self, size: Size, backend: Box<dyn PresentBackend>) {
        let resize_required = self.size() != size;
        let same_backend = backend.info().name == self.backend.info().name;
        if !resize_required && same_backend {
            return;
        }
        *self = Self::new(size, backend);
    }

    /// Current back-buffer size.
    pub fn size(&self) -> Size {
        self.swapchain
            .first()
            .map(crate::BackBuffer::size)
            .unwrap_or(Size::new(0, 0))
    }

    /// Drawing target for the frame being composed.
    pub fn canvas(&mut self) -> crate::GuiCanvas<'_> {
        self.swapchain[self.acquire_index].canvas()
    }

    /// Publish the whole acquired back buffer.
    pub fn present(&mut self) {
        self.present_region(Rect::new(0, 0, usize::MAX, usize::MAX));
    }

    /// Publish only `region` of the acquired back buffer (clipped).
    ///
    /// Cursor blinks and prompt edits present a few rows instead of the
    /// whole screen; the backend receives the same dirty-rect contract.
    pub fn present_region(&mut self, region: Rect) {
        if self.swapchain.is_empty() {
            return;
        }
        self.swapchain[self.acquire_index].present_region_to(self.backend.as_mut(), region);
        self.present_count = self.present_count.wrapping_add(1);
        self.acquire_index = (self.acquire_index + 1) % self.swapchain.len();
    }

    /// Snapshot of backend and swapchain state.
    pub fn diagnostics(&self) -> RendererDiagnostics {
        let info = self.backend.info();
        RendererDiagnostics {
            backend_name: info.name,
            api: info.api,
            present_mode: info.present_mode,
            frame_index: self.acquire_index,
            frames_in_flight: self.swapchain.len(),
        }
    }

    /// Total frames presented since construction.
    pub const fn present_count(&self) -> u64 {
        self.present_count
    }
}
