//! Concrete kernel drivers bound through the `orbita-drivers` framework.
//!
//! Each driver here demonstrates one device class through the shared
//! probe → attach → start lifecycle:
//!
//! - [`AhciStorageDriver`] — PCI storage (SATA/AHCI controller, port 0)
//! - [`EspFatDiskDriver`] — the firmware ESP drive (builtin ich9 SATA),
//!   mounted read-only as the `/pkg` delivery channel (FAT32)
//! - [`Ps2KeyboardDriver`] — legacy input device (i8042)
//!
//! Future drivers (e1000 networking, virtio-gpu presentation, NVMe)
//! register the same way; see `docs/drivers.md`.

use orbita_std::{String, Vec};
use orbita_drivers::{BindReport, DeviceClass, DeviceProbe, DriverManager};
use orbita_hw::{AhciDisk, E1000, PciBarKind, enable_bus_master};
use orbita_platform as platform;

use crate::disk::AhciSectorDisk;

/// PCI storage driver: claims the dedicated SATA/AHCI controller and
/// brings its disks online — port 0 carries the persistent OrbitaFS
/// disk, port 1 the firmware ESP (FAT delivery channel for `/pkg`).
pub(crate) struct AhciStorageDriver {
    disk: Option<AhciSectorDisk>,
    esp_disk: Option<AhciSectorDisk>,
    bound_at: Option<String>,
}

impl AhciStorageDriver {
    pub(crate) const fn new() -> Self {
        Self {
            disk: None,
            esp_disk: None,
            bound_at: None,
        }
    }

    /// Take the bound OrbitaFS disk (used by the filesystem layer).
    pub(crate) fn take_disk(&mut self) -> Option<AhciSectorDisk> {
        self.disk.take()
    }

    /// Take the bound ESP FAT disk (mounted read-only as `/pkg`).
    pub(crate) fn take_esp_disk(&mut self) -> Option<AhciSectorDisk> {
        self.esp_disk.take()
    }

    /// The builtin ich9 SATA controller (00:1f.2) belongs to the firmware;
    /// disks the kernel owns sit on the dedicated ich9-ahci device.
    fn is_builtin_esp_controller(device: &DeviceProbe) -> bool {
        device.bus == 0 && device.device == 0x1F && device.function == 2
    }
}

impl orbita_drivers::DriverTrait for AhciStorageDriver {
    fn name(&self) -> &'static str {
        "ahci-storage"
    }

    fn class(&self) -> DeviceClass {
        DeviceClass::Storage
    }

    fn probe(&self, device: &DeviceProbe) -> bool {
        if !device.is_pci_class(0x01, 0x06) {
            return false;
        }
        !AhciStorageDriver::is_builtin_esp_controller(device)
    }

    fn attach(&mut self, device: &DeviceProbe) -> Result<(), &'static str> {
        let abar = device
            .mmio_bars
            .iter()
            .flatten()
            .copied()
            .find(|base| *base != 0)
            .ok_or("ahci: no MMIO BAR (ABAR) mapped")?;
        platform::log_line_fmt(format_args!(
            "Orbita OS: ahci-storage found at {} abar=0x{:x}",
            device.location(),
            abar
        ));
        orbita_hw::set_debug_hook(|args| platform::log_line_fmt(args));

        enable_bus_master(orbita_hw::PciAddress {
            segment: 0,
            bus: device.bus,
            device: device.device,
            function: device.function,
        });

        // Port 0: the persistent OrbitaFS disk (required).
        let disk = AhciDisk::probe(abar, 0)
            .map_err(|_| "ahci: port 0 probe failed")?
            .ok_or("ahci: port 0 has no disk")?;
        self.disk = Some(AhciSectorDisk { inner: disk });

        // Port 1: the firmware ESP FAT drive (optional — the QEMU config
        // attaches it as bus 1 on the same controller).
        match AhciDisk::probe(abar, 1) {
            Ok(Some(esp)) => {
                self.esp_disk = Some(AhciSectorDisk { inner: esp });
                platform::log_line("Orbita OS: ahci esp disk found on port 1");
            }
            _ => {
                platform::log_line("Orbita OS: ahci esp disk not present (port 1)");
            }
        }

        self.bound_at = Some(device.location());
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        if self.disk.is_none() {
            return Err("ahci: start before attach");
        }
        Ok(())
    }

    fn as_any(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

/// PCI network driver: Intel e1000 (QEMU `-device e1000`), poll mode.
pub(crate) struct E1000NetDriver {
    nic: Option<E1000>,
}

impl E1000NetDriver {
    pub(crate) const fn new() -> Self {
        Self { nic: None }
    }

    /// Take the bound NIC (the kernel polls it from its main loop).
    pub(crate) fn take_nic(&mut self) -> Option<E1000> {
        self.nic.take()
    }
}

impl orbita_drivers::DriverTrait for E1000NetDriver {
    fn name(&self) -> &'static str {
        "e1000"
    }

    fn class(&self) -> DeviceClass {
        DeviceClass::Net
    }

    fn probe(&self, device: &DeviceProbe) -> bool {
        // 8086:100e — the QEMU e1000 default model.
        device.is_pci_id(0x8086, 0x100E)
    }

    fn attach(&mut self, device: &DeviceProbe) -> Result<(), &'static str> {
        let bar0 = device.pci_mmio_bar(0).ok_or("e1000: BAR0 (MMIO) not mapped")?;
        enable_bus_master(orbita_hw::PciAddress {
            segment: 0,
            bus: device.bus,
            device: device.device,
            function: device.function,
        });
        let nic = E1000::probe(bar0).map_err(|_| "e1000: controller init failed")?;
        platform::log_line_fmt(format_args!(
            "Orbita OS: e1000 up at {} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} link={}",
            device.location(),
            nic.mac()[0],
            nic.mac()[1],
            nic.mac()[2],
            nic.mac()[3],
            nic.mac()[4],
            nic.mac()[5],
            nic.link_up()
        ));
        self.nic = Some(nic);
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        if self.nic.is_none() {
            return Err("e1000: start before attach");
        }
        Ok(())
    }

    fn as_any(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

/// Legacy PS/2 keyboard input driver (i8042 controller).
pub(crate) struct Ps2KeyboardDriver {
    present: bool,
}

impl Ps2KeyboardDriver {
    pub(crate) const fn new() -> Self {
        Self { present: false }
    }
}

impl orbita_drivers::DriverTrait for Ps2KeyboardDriver {
    fn name(&self) -> &'static str {
        "ps2-keyboard"
    }

    fn class(&self) -> DeviceClass {
        DeviceClass::Input
    }

    fn probe(&self, device: &DeviceProbe) -> bool {
        device.legacy_id == "ps2-keyboard"
    }

    fn attach(&mut self, _device: &DeviceProbe) -> Result<(), &'static str> {
        match orbita_hw::probe_ps2_controller() {
            orbita_hw::Ps2Status::Present { config_byte } => {
                platform::log_line_fmt(format_args!(
                    "Orbita OS: ps2 controller present config=0x{:02x}",
                    config_byte
                ));
                self.present = true;
                Ok(())
            }
            orbita_hw::Ps2Status::NotResponding => Err("ps2: controller not responding"),
        }
    }

    fn start(&mut self) -> Result<(), &'static str> {
        if !self.present {
            return Err("ps2: start before attach");
        }
        if orbita_hw::initialize_keyboard() {
            platform::log_line("Orbita OS: ps2 keyboard initialized for polling input");
            Ok(())
        } else {
            Err("ps2: keyboard initialization failed")
        }
    }

    fn as_any(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

/// Build [`DeviceProbe`] observations for every discovered PCI device.
pub(crate) fn pci_probes(inventory: &orbita_hw::PciInventory) -> Vec<DeviceProbe> {
    inventory
        .devices()
        .iter()
        .map(|device| {
            let mut bars = [None; 6];
            for (index, slot) in device.bars.iter().enumerate() {
                if let Some(bar) = slot {
                    if matches!(bar.kind, PciBarKind::Mmio32 | PciBarKind::Mmio64) {
                        bars[index] = Some(bar.base);
                    }
                }
            }
            DeviceProbe::pci(
                device.address.bus,
                device.address.device,
                device.address.function,
                device.vendor_id.0,
                device.device_id.0,
                device.class_code.class,
                device.class_code.subclass,
                device.class_code.programming_interface,
                bars,
            )
        })
        .collect()
}

/// Register the built-in kernel drivers and run the bind pipeline over
/// `probes`. Returns the manager for later service access.
pub(crate) fn bind_builtin_drivers(probes: &[DeviceProbe]) -> (DriverManager, BindReport) {
    let mut manager = DriverManager::new();
    manager.register(orbita_std::Box::new(AhciStorageDriver::new()));
    manager.register(orbita_std::Box::new(Ps2KeyboardDriver::new()));
    manager.register(orbita_std::Box::new(E1000NetDriver::new()));
    let report = manager.bind_all(probes);
    (manager, report)
}

/// Print a compact bind summary to the boot console.
pub(crate) fn log_bind_report(report: &BindReport) {
    for record in &report.records {
        match &record.status {
            orbita_drivers::BindStatus::Bound => platform::log_line_fmt(format_args!(
                "Orbita OS: driver {} bound to {}",
                record.driver, record.device
            )),
            orbita_drivers::BindStatus::AttachFailed(reason) => platform::log_line_fmt(
                format_args!(
                    "Orbita OS: driver {} attach failed at {}: {}",
                    record.driver, record.device, reason
                ),
            ),
            orbita_drivers::BindStatus::StartFailed(reason) => platform::log_line_fmt(
                format_args!(
                    "Orbita OS: driver {} start failed at {}: {}",
                    record.driver, record.device, reason
                ),
            ),
        }
    }
}
