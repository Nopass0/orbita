//! Monitor device model.
//!
//! Bridges the PCI/GPU world (`orbita-hw`) with the display abstraction
//! (`orbita-video::screen`) without coupling the two crates: the kernel
//! feeds raw observations in, gets a typed monitor list out.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Where the information about this monitor came from.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MonitorSource {
    /// Firmware framebuffer (GOP) — identity unknown, timing known.
    Firmware,
    /// EDID block read over the display connector.
    Edid,
    /// Driver-reported virtual display (QEMU std/virtio-vga).
    DriverVirtual,
}

impl MonitorSource {
    pub fn label(self) -> &'static str {
        match self {
            MonitorSource::Firmware => "firmware",
            MonitorSource::Edid => "edid",
            MonitorSource::DriverVirtual => "driver-virtual",
        }
    }
}

/// One monitor as seen by the device manager.
#[derive(Debug, Clone)]
pub struct MonitorDevice {
    pub name: String,
    pub source: MonitorSource,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u8,
    /// PCI address of the GPU driving this monitor, when known
    /// (`bus:device.function` text).
    pub gpu: Option<String>,
}

/// Inventory of monitors attached to the system.
#[derive(Debug, Default)]
pub struct MonitorList {
    pub monitors: Vec<MonitorDevice>,
}

impl MonitorList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the firmware framebuffer as the primary monitor.
    pub fn push_firmware(&mut self, width: u32, height: u32) {
        self.monitors.push(MonitorDevice {
            name: String::from("firmware-primary"),
            source: MonitorSource::Firmware,
            width,
            height,
            refresh_hz: 60,
            gpu: None,
        });
    }

    /// Records a driver/virtual display, e.g. QEMU's std VGA.
    pub fn push_virtual(&mut self, gpu: &str, width: u32, height: u32) {
        self.monitors.push(MonitorDevice {
            name: String::from("virtual-0"),
            source: MonitorSource::DriverVirtual,
            width,
            height,
            refresh_hz: 60,
            gpu: Some(String::from(gpu)),
        });
    }

    /// Records an EDID-identified monitor.
    pub fn push_edid(&mut self, name: &str, gpu: &str, width: u32, height: u32, refresh_hz: u8) {
        self.monitors.push(MonitorDevice {
            name: String::from(name),
            source: MonitorSource::Edid,
            width,
            height,
            refresh_hz,
            gpu: Some(String::from(gpu)),
        });
    }

    pub fn len(&self) -> usize {
        self.monitors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.monitors.is_empty()
    }

    /// One-line summary for logs.
    pub fn summary(&self) -> String {
        let mut out = format!("{} monitor(s)", self.len());
        for m in &self.monitors {
            out.push_str(&format!(
                " [{} {} {}x{}@{} gpu={}]",
                m.name,
                m.source.label(),
                m.width,
                m.height,
                m.refresh_hz,
                m.gpu.as_deref().unwrap_or("none")
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_and_edid_monitors() {
        let mut list = MonitorList::new();
        list.push_firmware(1920, 1080);
        list.push_edid("AAA-1234", "00:01.0", 640, 480, 60);
        assert_eq!(list.len(), 2);
        let s = list.summary();
        assert!(s.contains("firmware-primary"));
        assert!(s.contains("AAA-1234"));
        assert!(s.contains("00:01.0"));
    }
}
