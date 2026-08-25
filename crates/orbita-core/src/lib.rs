#![no_std]

//! Shared desktop runtime state: built-in app registry, chrome/workspace
//! state machines, graphics-backend preference, and the runtime event
//! buffer. The kernel and the desktop renderer both consume this crate,
//! so it stays free of rendering and I/O code.

extern crate alloc;

mod apps;
mod runtime;
mod services;

use orbita_proto::BootInfo;

pub use apps::{AppLaunchState, BuiltinApp, BuiltinAppIcon, builtin_apps, builtin_apps_manifest};
pub use runtime::{
    DesktopChromePanel, DesktopChromeState, DesktopFocusTarget, DesktopPointerState,
    DesktopSessionState, DesktopWorkspaceState, GraphicsBackend, RuntimeEventBuffer,
    SettingsSection,
};
pub use services::{
    BuiltinService, BuiltinServiceState, RuntimeServiceHealth, ServiceRuntimeRecord,
    builtin_services, builtin_services_manifest, runtime_services_manifest,
};

#[derive(Debug, Copy, Clone)]
pub struct BootSummary {
    pub total_memory_bytes: u64,
    pub usable_memory_bytes: u64,
    pub framebuffer_width: usize,
    pub framebuffer_height: usize,
    pub framebuffer_stride: usize,
}

impl BootSummary {
    pub fn from_boot_info(info: &BootInfo) -> Self {
        let stats = info.memory_statistics();
        Self {
            total_memory_bytes: stats.total_bytes,
            usable_memory_bytes: stats.usable_bytes,
            framebuffer_width: info.framebuffer.width,
            framebuffer_height: info.framebuffer.height,
            framebuffer_stride: info.framebuffer.stride,
        }
    }

    pub fn total_memory_mebibytes(&self) -> u64 {
        self.total_memory_bytes / (1024 * 1024)
    }

    pub fn usable_memory_mebibytes(&self) -> u64 {
        self.usable_memory_bytes / (1024 * 1024)
    }
}
