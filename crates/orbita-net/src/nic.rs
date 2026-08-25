//! Network interface controller device model.
//!
//! Matches PCI vendor:device pairs to NIC driver kinds and tracks link
//! state. Actual DMA register programming lives with the drivers; this is
//! the contract and inventory layer.

use crate::ethernet::MacAddress;
use orbita_std::{String, format};

/// Which driver backend should handle a NIC.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum NicDriverKind {
    IntelE1000,
    VirtioNet,
    Realtek8139,
    PcNet32,
    Loopback,
    Unknown,
}

impl NicDriverKind {
    pub fn label(self) -> &'static str {
        match self {
            NicDriverKind::IntelE1000 => "e1000",
            NicDriverKind::VirtioNet => "virtio-net",
            NicDriverKind::Realtek8139 => "rtl8139",
            NicDriverKind::PcNet32 => "pcnet32",
            NicDriverKind::Loopback => "loopback",
            NicDriverKind::Unknown => "unknown",
        }
    }

    /// Matches a PCI vendor:device pair.
    pub fn from_pci(vendor_id: u16, device_id: u16) -> Self {
        match (vendor_id, device_id) {
            (0x8086, 0x100E) | (0x8086, 0x100F) | (0x8086, 0x10D3) => NicDriverKind::IntelE1000,
            (0x1AF4, 0x1000) | (0x1AF4, 0x1041) => NicDriverKind::VirtioNet,
            (0x10EC, 0x8139) => NicDriverKind::Realtek8139,
            (0x1022, 0x2000) => NicDriverKind::PcNet32,
            _ => NicDriverKind::Unknown,
        }
    }
}

/// Link state of a NIC.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum NicStatus {
    /// No cable / no carrier.
    Down,
    /// Carrier present.
    Up { speed_mbps: u16 },
}

impl NicStatus {
    pub fn is_up(&self) -> bool {
        matches!(self, NicStatus::Up { .. })
    }

    pub fn label(&self) -> String {
        match self {
            NicStatus::Down => String::from("down"),
            NicStatus::Up { speed_mbps } => format!("up@{speed_mbps}Mbps"),
        }
    }
}

/// One NIC in the system inventory.
#[derive(Debug, Clone)]
pub struct NicInfo {
    /// PCI address text (`bus:device.function`) or `loopback`.
    pub pci_address: String,
    pub driver: NicDriverKind,
    pub mac: MacAddress,
    pub status: NicStatus,
}

impl NicInfo {
    /// A virtual loopback NIC.
    pub fn loopback() -> Self {
        Self {
            pci_address: String::from("loopback"),
            driver: NicDriverKind::Loopback,
            mac: MacAddress::ZERO,
            status: NicStatus::Up { speed_mbps: 0 },
        }
    }

    /// One-line summary for logs.
    pub fn summary(&self) -> String {
        format!(
            "nic {} [{}] mac={} {}",
            self.pci_address,
            self.driver.label(),
            self.mac.text(),
            self.status.label()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_matching() {
        assert_eq!(NicDriverKind::from_pci(0x8086, 0x100E), NicDriverKind::IntelE1000);
        assert_eq!(NicDriverKind::from_pci(0x1AF4, 0x1000), NicDriverKind::VirtioNet);
        assert_eq!(NicDriverKind::from_pci(0x10EC, 0x8139), NicDriverKind::Realtek8139);
        assert_eq!(NicDriverKind::from_pci(0xDEAD, 0xBEEF), NicDriverKind::Unknown);
    }

    #[test]
    fn loopback_summary() {
        let nic = NicInfo::loopback();
        assert!(nic.summary().contains("loopback"));
        assert!(nic.status.is_up());
    }
}
