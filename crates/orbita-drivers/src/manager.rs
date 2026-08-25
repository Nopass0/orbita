//! System device manager.
//!
//! Aggregates raw PCI observations into a typed, loggable inventory of
//! everything the kernel knows about: GPUs (and their boards), NICs,
//! storage/audio/USB controllers, bridges, and monitors. The kernel maps
//! `orbita_hw::PciInventory` entries into `PciObservation` records and the
//! manager classifies and summarizes them.

use crate::monitor::MonitorList;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// A raw PCI observation the kernel feeds in.
#[derive(Debug, Copy, Clone)]
pub struct PciObservation {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

impl PciObservation {
    /// `bus:device.function` text.
    pub fn address(&self) -> String {
        format!("{:02x}:{:02x}.{}", self.bus, self.device, self.function)
    }
}

/// Classified system device kind.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SystemDeviceKind {
    Gpu,
    Network,
    Storage,
    Audio,
    Usb,
    Bridge,
    Other,
}

impl SystemDeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            SystemDeviceKind::Gpu => "gpu",
            SystemDeviceKind::Network => "network",
            SystemDeviceKind::Storage => "storage",
            SystemDeviceKind::Audio => "audio",
            SystemDeviceKind::Usb => "usb",
            SystemDeviceKind::Bridge => "bridge",
            SystemDeviceKind::Other => "other",
        }
    }
}

/// One classified device in the system inventory.
#[derive(Debug, Clone)]
pub struct SystemDevice {
    pub kind: SystemDeviceKind,
    pub address: String,
    pub vendor_id: u16,
    pub device_id: u16,
    /// Human guess of the board/chip behind the ids, when recognized.
    pub identity: &'static str,
}

/// The full device inventory.
#[derive(Debug, Default)]
pub struct DeviceManager {
    pub devices: Vec<SystemDevice>,
    pub monitors: MonitorList,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Classifies one PCI observation and adds it to the inventory.
    pub fn observe_pci(&mut self, obs: PciObservation) {
        let kind = classify(obs.class, obs.subclass);
        self.devices.push(SystemDevice {
            kind,
            address: obs.address(),
            vendor_id: obs.vendor_id,
            device_id: obs.device_id,
            identity: identify(obs.vendor_id, obs.device_id),
        });
    }

    /// Classifies a batch of observations.
    pub fn observe_all(&mut self, observations: &[PciObservation]) {
        for &obs in observations {
            self.observe_pci(obs);
        }
    }

    pub fn count(&self, kind: SystemDeviceKind) -> usize {
        self.devices.iter().filter(|d| d.kind == kind).count()
    }

    pub fn total(&self) -> usize {
        self.devices.len()
    }

    /// Multi-line inventory report for the boot log.
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "devices: total={} gpu={} net={} storage={} audio={} usb={} bridge={}",
            self.total(),
            self.count(SystemDeviceKind::Gpu),
            self.count(SystemDeviceKind::Network),
            self.count(SystemDeviceKind::Storage),
            self.count(SystemDeviceKind::Audio),
            self.count(SystemDeviceKind::Usb),
            self.count(SystemDeviceKind::Bridge),
        ));
        for d in &self.devices {
            lines.push(format!(
                "  {} {} [{:04x}:{:04x}] {}",
                d.address,
                d.kind.label(),
                d.vendor_id,
                d.device_id,
                d.identity
            ));
        }
        lines.push(self.monitors.summary());
        lines
    }
}

/// PCI class/subclass → device kind (top-level classes).
fn classify(class: u8, subclass: u8) -> SystemDeviceKind {
    match class {
        0x00 if subclass == 0x01 => SystemDeviceKind::Bridge, // host bridge
        0x01 => SystemDeviceKind::Storage,
        0x02 => SystemDeviceKind::Network,
        0x03 => SystemDeviceKind::Gpu,
        0x04 => SystemDeviceKind::Audio,
        0x0C if subclass == 0x03 => SystemDeviceKind::Usb,
        0x06 => SystemDeviceKind::Bridge,
        _ => SystemDeviceKind::Other,
    }
}

/// Recognizes well-known vendor:device pairs (QEMU/virtio and common HW).
fn identify(vendor: u16, device: u16) -> &'static str {
    match (vendor, device) {
        (0x1234, 0x1111) => "qemu std vga",
        (0x1B36, 0x0100) => "qxl virtual gpu",
        (0x1AF4, 0x1050) => "virtio-gpu modern",
        (0x8086, 0x100E) => "intel e1000 nic",
        (0x8086, 0x100F) => "intel e1000 nic (82545em)",
        (0x1AF4, 0x1000) => "virtio-net legacy",
        (0x1AF4, 0x1041) => "virtio-net modern",
        (0x10EC, 0x8139) => "realtek rtl8139 nic",
        (0x1022, 0x2000) => "amd pcnet32 nic",
        (0x8086, 0x2922) => "intel ahci sata",
        (0x8086, 0x7010) => "intel ide piix",
        (0x1AF4, 0x1001) => "virtio-blk legacy",
        (0x1AF4, 0x1042) => "virtio-blk modern",
        (0x1AF4, 0x1003) => "virtio-console",
        (0x8086, 0x2668) => "intel hd audio",
        (0x8086, 0x2934) => "intel usb uhci",
        (0x8086, 0x2935) => "intel usb uhci",
        (0x8086, 0x1237) => "intel 440fx bridge",
        (0x8086, 0x29C0) => "intel q35 bridge",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(bus: u8, dev: u8, vendor: u16, device: u16, class: u8) -> PciObservation {
        PciObservation {
            bus,
            device: dev,
            function: 0,
            vendor_id: vendor,
            device_id: device,
            class,
            subclass: 0,
            programming_interface: 0,
        }
    }

    #[test]
    fn classifies_and_identifies() {
        let mut mgr = DeviceManager::new();
        mgr.observe_pci(obs(0, 1, 0x1234, 0x1111, 0x03)); // qemu vga
        mgr.observe_pci(obs(0, 2, 0x8086, 0x100E, 0x02)); // e1000
        mgr.observe_pci(obs(0, 31, 0x8086, 0x2922, 0x01)); // ahci
        assert_eq!(mgr.total(), 3);
        assert_eq!(mgr.count(SystemDeviceKind::Gpu), 1);
        assert_eq!(mgr.count(SystemDeviceKind::Network), 1);
        assert_eq!(mgr.count(SystemDeviceKind::Storage), 1);
        let report = mgr.report_lines().join("\n");
        assert!(report.contains("qemu std vga"));
        assert!(report.contains("intel e1000 nic"));
    }

    #[test]
    fn address_text() {
        assert_eq!(obs(0, 1, 0, 0, 0).address(), "00:01.0");
    }
}
