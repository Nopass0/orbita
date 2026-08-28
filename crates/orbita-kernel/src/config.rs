//! Live system configuration (`/etc/orbita.conf`): defaults, parsing, and application.


extern crate alloc;

use alloc::vec::Vec;
use orbita_fs::MemoryVolume;
use orbita_std::String;
use crate::console::*;

pub(crate) const ORBITA_CONF: &str = "/etc/orbita.conf";

pub(crate) fn orbita_conf_default() -> &'static str {
    "hostname=orbita\nmodules=full\nboot_splash=on\npaging_dry_run=on\npaging_cr3=on\n"
}

/// Parses "key=value" lines.
pub(crate) fn parse_conf(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((String::from(key.trim()), String::from(value.trim())))
        })
        .collect()
}

/// Applies configuration values to the running system.
/// Live configuration state parsed from /etc/orbita.conf.
pub(crate) static CONF_FLAGS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub(crate) fn apply_orbita_conf(text: &str, console: &mut BootConsole, ram_fs: &mut MemoryVolume) {
    for (key, value) in parse_conf(text) {
        if key == "hostname" && !value.is_empty() {
            console.hostname = value.clone();
        }
    }
    // Record boolean flags for kernel-side gating (smp=on, …).
    let mut bits = 0u64;
    for (key, value) in parse_conf(text) {
        if value == "on" || value == "true" || value == "1" {
            match key.as_str() {
                "smp" => bits |= 1,
                "hot_reload" => bits |= 2,
                _ => {}
            }
        }
    }
    CONF_FLAGS.store(bits, core::sync::atomic::Ordering::SeqCst);
    // Mirror the live config into the RAM volume so `cat /etc/orbita.conf`
    // shows what the kernel actually parsed and the Files app can open it.
    let _ = ram_fs.create_file_path("/etc/orbita.conf", text.as_bytes());
}

/// Reads a boolean flag from the live configuration.
pub(crate) fn persistent_conf_flag(key: &str) -> bool {
    let bits = CONF_FLAGS.load(core::sync::atomic::Ordering::SeqCst);
    match key {
        "smp" => bits & 1 != 0,
        "hot_reload" => bits & 2 != 0,
        _ => false,
    }
}

pub(crate) fn config_contains(fs: &mut MemoryVolume, path: &str, needle: &str) -> bool {
    fs.read_file_path(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).contains(needle))
        .unwrap_or(false)
}

pub(crate) fn toggle_config_value(fs: &mut MemoryVolume, path: &str, from: &str, to: &str) -> bool {
    let Ok(bytes) = fs.read_file_path(path) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes);
    if !text.contains(from) {
        return false;
    }
    let updated = text.replacen(from, to, 1);
    fs.create_file_path(path, updated.as_bytes()).is_ok()
}

/// Graphics backend preference from `/etc/orbita.conf` (`gfx=<name>`).
///
/// `vulkan` (or any driver-registered engine name) is honoured when the
/// engine is registered; unknown names fall back to the software
/// framebuffer at backend creation time.
pub(crate) fn preferred_graphics_backend(text: &str) -> orbita_core::GraphicsBackend {
    for (key, value) in parse_conf(text) {
        if key == "gfx" {
            return match value.as_str() {
                "vulkan" => orbita_core::GraphicsBackend::Vulkan,
                _ => orbita_core::GraphicsBackend::SoftwareFramebuffer,
            };
        }
    }
    orbita_core::GraphicsBackend::SoftwareFramebuffer
}

/// Whether `/etc/orbita.conf` enables the stage-A paging dry run
/// (`paging_dry_run=on`): build an identity map in 2 MiB huge pages
/// without switching CR3. On by default for fresh installs.
pub(crate) fn wants_paging_dry_run(text: &str) -> bool {
    parse_conf(text)
        .iter()
        .any(|(key, value)| key == "paging_dry_run" && (value == "on" || value == "1"))
}

/// Whether `/etc/orbita.conf` enables switching CR3 to the kernel-built
/// identity map (`paging_cr3=on`). On by default since stage-A portion 5:
/// the map covers the whole low 4 GiB plus every descriptor above it, and
/// QEMU smoke (cold + warm boot) passes on kernel tables.
pub(crate) fn wants_paging_cr3(text: &str) -> bool {
    parse_conf(text)
        .iter()
        .any(|(key, value)| key == "paging_cr3" && (value == "on" || value == "1"))
}
