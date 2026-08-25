//! Screen and monitor abstractions.
//!
//! A `DisplayInfo` describes one attached monitor (physical or virtual),
//! `ModeInfo` one graphics mode, and `MonitorInventory` collects the
//! displays visible to the kernel: the GOP-backed primary plus any
//! EDID-identified monitors reported by drivers later on.

use crate::edid::{self, EdidInfo};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Physical connector a monitor is attached through.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Connector {
    Vga,
    Dvi,
    Hdmi,
    DisplayPort,
    UsbC,
    Virtual,
}

impl Connector {
    pub fn label(self) -> &'static str {
        match self {
            Connector::Vga => "vga",
            Connector::Dvi => "dvi",
            Connector::Hdmi => "hdmi",
            Connector::DisplayPort => "dp",
            Connector::UsbC => "usb-c",
            Connector::Virtual => "virtual",
        }
    }
}

/// One graphics mode of a display.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ModeInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u8,
}

impl ModeInfo {
    pub const fn new(width: u32, height: u32, refresh_hz: u8) -> Self {
        Self {
            width,
            height,
            refresh_hz,
        }
    }
}

/// One attached monitor.
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// Human-readable label, e.g. "AAA-1234" from EDID or "GOP-0".
    pub name: String,
    pub connector: Connector,
    pub preferred: ModeInfo,
    /// Active mode (what the kernel is currently scanning out).
    pub active: ModeInfo,
    /// True when the display was identified via EDID.
    pub edid_identified: bool,
}

impl DisplayInfo {
    /// The primary display backing the kernel framebuffer: reported by the
    /// firmware (GOP) before any real driver takes over. Connector is
    /// `Virtual` unless the platform knows better.
    pub fn firmware_primary(width: u32, height: u32) -> Self {
        Self {
            name: String::from("GOP-0"),
            connector: Connector::Virtual,
            preferred: ModeInfo::new(width, height, 60),
            active: ModeInfo::new(width, height, 60),
            edid_identified: false,
        }
    }

    /// Builds a display from a parsed EDID block.
    pub fn from_edid(connector: Connector, info: &EdidInfo) -> Self {
        let timing = info
            .preferred
            .map(|t| ModeInfo::new(t.width as u32, t.height as u32, t.refresh_hz))
            .unwrap_or(ModeInfo::new(1024, 768, 60));
        Self {
            name: format!(
                "{}-{:04X}",
                edid::manufacturer_text(&info.manufacturer_id),
                info.product_code
            ),
            connector,
            preferred: timing,
            active: timing,
            edid_identified: true,
        }
    }
}

/// All displays known to the kernel right now.
#[derive(Debug, Default)]
pub struct MonitorInventory {
    pub displays: Vec<DisplayInfo>,
}

impl MonitorInventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds the inventory with the firmware framebuffer display.
    pub fn with_firmware_primary(width: u32, height: u32) -> Self {
        let mut inv = Self::new();
        inv.displays.push(DisplayInfo::firmware_primary(width, height));
        inv
    }

    /// Adds an EDID-identified monitor on a connector.
    pub fn push_edid(&mut self, connector: Connector, info: &EdidInfo) {
        self.displays.push(DisplayInfo::from_edid(connector, info));
    }

    pub fn len(&self) -> usize {
        self.displays.len()
    }

    pub fn is_empty(&self) -> bool {
        self.displays.is_empty()
    }

    /// One-line summary for logs, e.g. `2 displays: [GOP-0 virtual 1920x1080@60] [AAA-1234 hdmi 640x480@60]`.
    pub fn summary(&self) -> String {
        let mut out = format!("{} display(s)", self.len());
        for d in &self.displays {
            out.push_str(&format!(
                " [{} {} {}x{}@{}{}]",
                d.name,
                d.connector.label(),
                d.active.width,
                d.active.height,
                d.active.refresh_hz,
                if d.edid_identified { " edid" } else { "" }
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use crate::edid;

    fn sample_edid_bytes() -> Vec<u8> {
        let mut d = vec![0u8; 128];
        d[0] = 0x00;
        d[1..7].fill(0xFF);
        d[7] = 0x00;
        d[8] = 0x04; // "AAA"
        d[9] = 0x21;
        d[10..12].copy_from_slice(&0x1234u16.to_le_bytes());
        let t = &mut d[0x36..0x36 + 18];
        t[0] = 0x39;
        t[1] = 0x97;
        t[2] = 0x80; // 640 low byte (640 = 0x280)
        t[3] = 0x20;
        t[4] = 0x20;
        t[5] = 0xE0; // 480
        t[6] = 0x12;
        t[7] = 0x10;
        let sum: u8 = d[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        d[127] = sum.wrapping_neg();
        d
    }

    #[test]
    fn firmware_primary_inventory() {
        let inv = MonitorInventory::with_firmware_primary(1920, 1080);
        assert_eq!(inv.len(), 1);
        assert!(inv.summary().contains("GOP-0"));
        assert!(inv.summary().contains("1920x1080@60"));
    }

    #[test]
    fn edid_monitor_added() {
        let bytes = sample_edid_bytes();
        let info = edid::parse(&bytes).expect("edid");
        let mut inv = MonitorInventory::with_firmware_primary(800, 600);
        inv.push_edid(Connector::Hdmi, &info);
        assert_eq!(inv.len(), 2);
        assert!(inv.summary().contains("AAA-1234"));
        assert!(inv.summary().contains("hdmi"));
    }
}
