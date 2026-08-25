//! The Orbita driver contract: probe → attach → start over a dynamic
//! registry.
//!
//! # Writing a driver
//!
//! 1. Implement [`Driver`] for your device (PCI or legacy).
//! 2. Register an instance with [`DriverManager::register`] during kernel
//!    boot (or later, from a loadable module).
//! 3. The kernel feeds every discovered device into
//!    [`DriverManager::bind_all`]; your `probe` claims matching devices,
//!    `attach` binds hardware, `start` brings them online.
//!
//! ```ignore
//! struct AhciPciDriver { disk: Option<AhciDisk> }
//!
//! impl Driver for AhciPciDriver {
//!     fn name(&self) -> &'static str { "ahci" }
//!     fn class(&self) -> DeviceClass { DeviceClass::Storage }
//!     fn probe(&self, dev: &DeviceProbe) -> bool {
//!         dev.is_pci_class(0x01, 0x06) // mass storage / SATA-AHCI
//!     }
//!     fn attach(&mut self, dev: &DeviceProbe) -> Result<(), &'static str> {
//!         let abar = dev.pci_mmio_bar(5).ok_or("ahci: no ABAR")?;
//!         self.disk = AhciDisk::probe(abar, 0).map_err(|_| "probe failed")?;
//!         Ok(())
//!     }
//!     fn start(&mut self) -> Result<(), &'static str> { Ok(()) }
//! }
//! ```
//!
//! Drivers live anywhere in the workspace; only the [`Driver`] trait and
//! the probe types come from this crate, so `orbita-hw` primitives and
//! kernel services can be combined freely in the implementation.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::DeviceClass;

/// One discovered device offered to the drivers.
#[derive(Debug, Copy, Clone)]
pub struct DeviceProbe {
    /// PCI address (bus, device, function); zeros for legacy devices.
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub programming_interface: u8,
    /// Decoded PCI BARs (`None` for unmapped slots and legacy devices).
    pub mmio_bars: [Option<u64>; 6],
    /// Non-PCI identification for legacy devices (`ps2-keyboard`, ...).
    pub legacy_id: &'static str,
}

impl DeviceProbe {
    /// A PCI device observation.
    #[allow(clippy::too_many_arguments)]
    pub fn pci(
        bus: u8,
        device: u8,
        function: u8,
        vendor_id: u16,
        device_id: u16,
        class: u8,
        subclass: u8,
        programming_interface: u8,
        mmio_bars: [Option<u64>; 6],
    ) -> Self {
        Self {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            class,
            subclass,
            programming_interface,
            mmio_bars,
            legacy_id: "",
        }
    }

    /// A legacy (non-PCI) device observation.
    pub fn legacy(legacy_id: &'static str) -> Self {
        Self {
            bus: 0,
            device: 0,
            function: 0,
            vendor_id: 0,
            device_id: 0,
            class: 0,
            subclass: 0,
            programming_interface: 0,
            mmio_bars: [None; 6],
            legacy_id,
        }
    }

    /// Matches a PCI class/subclass pair (programming interface ignored).
    pub fn is_pci_class(&self, class: u8, subclass: u8) -> bool {
        self.legacy_id.is_empty() && self.class == class && self.subclass == subclass
    }

    /// Matches a PCI vendor/device pair.
    pub fn is_pci_id(&self, vendor_id: u16, device_id: u16) -> bool {
        self.legacy_id.is_empty() && self.vendor_id == vendor_id && self.device_id == device_id
    }

    /// Base address of the n-th MMIO BAR, if mapped.
    pub fn pci_mmio_bar(&self, index: usize) -> Option<u64> {
        self.mmio_bars.get(index).copied().flatten()
    }

    /// Human-readable device location (`00:1f.2` or `legacy:<id>`).
    pub fn location(&self) -> String {
        if self.legacy_id.is_empty() {
            String::from(alloc::format!(
                "{:02x}:{:02x}.{}",
                self.bus, self.device, self.function
            ))
        } else {
            String::from(alloc::format!("legacy:{}", self.legacy_id))
        }
    }
}

/// The driver lifecycle contract.
pub trait Driver {
    /// Stable registry name (`"ahci"`, `"e1000"`, ...).
    fn name(&self) -> &'static str;
    /// Device class served by this driver.
    fn class(&self) -> DeviceClass;
    /// Does this driver want `device`? Called for every discovered device.
    fn probe(&self, device: &DeviceProbe) -> bool;
    /// Bind to the probed device (map BARs, claim ports, ...).
    fn attach(&mut self, device: &DeviceProbe) -> Result<(), &'static str>;
    /// Bring the bound device into working state.
    fn start(&mut self) -> Result<(), &'static str>;
    /// Optional interrupt service entry point.
    fn handle_irq(&mut self) {}
    /// Quiesce the device.
    fn stop(&mut self) {}
    /// Downcast support so the kernel can retrieve concrete instances.
    fn as_any(&mut self) -> &mut dyn core::any::Any;
}

/// Outcome of one bind attempt.
#[derive(Debug, Clone)]
pub struct BindRecord {
    pub driver: &'static str,
    pub device: String,
    pub status: BindStatus,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BindStatus {
    /// probe matched, attach + start succeeded.
    Bound,
    /// probe matched but attach failed.
    AttachFailed(String),
    /// probe matched, attach succeeded, start failed.
    StartFailed(String),
}

/// Result of a full [`DriverManager::bind_all`] pass.
#[derive(Debug, Default)]
pub struct BindReport {
    pub records: Vec<BindRecord>,
}

impl BindReport {
    /// Number of drivers that reached the `Bound` state.
    pub fn bound(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.status == BindStatus::Bound)
            .count()
    }
}

/// Owns the registered drivers and runs the bind pipeline.
pub struct DriverManager {
    drivers: Vec<Box<dyn Driver>>,
    irq_vectors: Vec<(String, u8)>,
}

impl DriverManager {
    pub fn new() -> Self {
        Self {
            drivers: Vec::new(),
            irq_vectors: Vec::new(),
        }
    }

    /// Register a driver instance. Registration order is the probe order.
    pub fn register(&mut self, driver: Box<dyn Driver>) {
        self.drivers.push(driver);
    }

    /// Number of registered drivers.
    pub fn len(&self) -> usize {
        self.drivers.len()
    }

    /// Whether no driver is registered.
    pub fn is_empty(&self) -> bool {
        self.drivers.is_empty()
    }

    /// Offer every device to every driver; first driver that probes and
    /// attaches successfully wins the device.
    pub fn bind_all(&mut self, devices: &[DeviceProbe]) -> BindReport {
        let mut report = BindReport::default();
        let mut taken = alloc::vec![false; devices.len()];
        for driver in &mut self.drivers {
            for (index, device) in devices.iter().enumerate() {
                if taken[index] || !driver.probe(device) {
                    continue;
                }
                let record = match driver.attach(device) {
                    Err(reason) => BindRecord {
                        driver: driver.name(),
                        device: device.location(),
                        status: BindStatus::AttachFailed(String::from(reason)),
                    },
                    Ok(()) => match driver.start() {
                        Ok(()) => {
                            taken[index] = true;
                            BindRecord {
                                driver: driver.name(),
                                device: device.location(),
                                status: BindStatus::Bound,
                            }
                        }
                        Err(reason) => BindRecord {
                            driver: driver.name(),
                            device: device.location(),
                            status: BindStatus::StartFailed(String::from(reason)),
                        },
                    },
                };
                report.records.push(record);
            }
        }
        report
    }

    /// Assign an IRQ vector to a named driver (kernel-side routing table).
    pub fn assign_irq(&mut self, driver_name: &str, vector: u8) {
        self.irq_vectors.push((String::from(driver_name), vector));
    }

    /// Interrupt dispatch to the driver that owns `vector` — the kernel
    /// routes hardware IRQs here after its own bookkeeping.
    pub fn dispatch_irq(&mut self, vector: u8) {
        for (name, v) in self.irq_vectors.clone() {
            if v == vector {
                if let Some(driver) = self.by_name(&name) {
                    driver.handle_irq();
                }
                return;
            }
        }
    }

    /// Borrow a registered driver by name for service access.
    pub fn by_name(&mut self, name: &str) -> Option<&mut dyn Driver> {
        self.drivers
            .iter_mut()
            .map(|d| d.as_mut() as &mut dyn Driver)
            .find(|d| d.name() == name)
    }

    /// Borrow a registered driver by name for downcasting to its concrete
    /// type (`driver_manager.by_name_any("ahci-storage")
    /// .and_then(|any| any.downcast_mut::<AhciStorageDriver>())`).
    pub fn by_name_any(&mut self, name: &str) -> Option<&mut dyn core::any::Any> {
        self.drivers
            .iter_mut()
            .find(|d| d.name() == name)
            .map(|d| d.as_any())
    }
}

impl Default for DriverManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RamDiskDriver {
        started: bool,
    }

    impl Driver for RamDiskDriver {
        fn name(&self) -> &'static str {
            "test-ramdisk"
        }
        fn class(&self) -> DeviceClass {
            DeviceClass::Storage
        }
        fn probe(&self, device: &DeviceProbe) -> bool {
            device.legacy_id == "test-disk"
        }
        fn attach(&mut self, _device: &DeviceProbe) -> Result<(), &'static str> {
            Ok(())
        }
        fn start(&mut self) -> Result<(), &'static str> {
            self.started = true;
            Ok(())
        }
        fn as_any(&mut self) -> &mut dyn core::any::Any {
            self
        }
    }

    #[test]
    fn bind_pipeline_binds_legacy_driver() {
        let mut manager = DriverManager::new();
        manager.register(Box::new(RamDiskDriver { started: false }));
        let report = manager.bind_all(&[DeviceProbe::legacy("test-disk")]);
        assert_eq!(report.bound(), 1);
        let driver = manager.by_name("test-ramdisk").unwrap();
        let _ = driver.name();
    }

    #[test]
    fn probe_mismatch_leaves_device_untaken() {
        let mut manager = DriverManager::new();
        manager.register(Box::new(RamDiskDriver { started: false }));
        let report = manager.bind_all(&[DeviceProbe::legacy("other-device")]);
        assert_eq!(report.bound(), 0);
        assert!(report.records.is_empty());
    }
}
