use alloc::{string::String, vec::Vec};
use core::fmt;
use core::fmt::Write;

use orbita_arch_x86_64::io::Port;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PciVendorId(pub u16);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PciDeviceId(pub u16);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PciAddress {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PciClassCode {
    pub class: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PciBarKind {
    Mmio32,
    Mmio64,
    IoPort,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PciBar {
    pub kind: PciBarKind,
    pub base: u64,
    pub size: u64,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PciCapabilityId {
    Msi,
    Msix,
    VendorSpecific,
    Other(u8),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PciCapability {
    pub offset: u8,
    pub id: PciCapabilityId,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum VirtioTransport {
    Legacy,
    Modern,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum VirtioDeviceType {
    Network,
    Block,
    Console,
    Entropy,
    Balloon,
    ScsiHost,
    Gpu,
    Input,
    Socket,
    Sound,
    Unknown(u16),
}

const CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const CONFIG_DATA_PORT: u16 = 0xCFC;
const STATUS_CAPABILITIES_LIST: u16 = 1 << 4;
const PCI_CAP_ID_MSI: u8 = 0x05;
const PCI_CAP_ID_VENDOR: u8 = 0x09;
const PCI_CAP_ID_MSIX: u8 = 0x11;
const PCI_HEADER_TYPE_MASK: u8 = 0x7F;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor_id: PciVendorId,
    pub device_id: PciDeviceId,
    pub class_code: PciClassCode,
    pub revision: u8,
    pub header_type: u8,
    pub bars: [Option<PciBar>; 6],
    pub capabilities: [Option<PciCapability>; 8],
    pub capability_count: usize,
    pub msi_capable: bool,
    pub msix_capable: bool,
    pub virtio_transport: Option<VirtioTransport>,
    pub virtio_device_type: Option<VirtioDeviceType>,
}

impl PciDevice {
    pub fn address_string(&self) -> String {
        let mut out = String::new();
        let _ = write!(
            &mut out,
            "{:04x}:{:02x}:{:02x}.{}",
            self.address.segment, self.address.bus, self.address.device, self.address.function
        );
        out
    }

    pub fn vendor_device_id(&self) -> (u16, u16) {
        (self.vendor_id.0, self.device_id.0)
    }

    pub fn class_name(&self) -> &'static str {
        match (self.class_code.class, self.class_code.subclass) {
            (0x01, 0x06) => "sata-ahci",
            (0x01, 0x08) => "nvme",
            (0x02, 0x00) => "ethernet",
            (0x03, 0x00) => "vga-compatible",
            (0x03, 0x80) => "display-controller",
            (0x04, 0x01) => "audio-device",
            (0x06, 0x00) => "host-bridge",
            (0x06, 0x01) => "isa-bridge",
            (0x06, 0x04) => "pci-bridge",
            (0x0C, 0x03) => "usb-controller",
            _ => "generic",
        }
    }

    pub fn is_bridge(&self) -> bool {
        matches!((self.class_code.class, self.class_code.subclass), (0x06, 0x04))
    }

    pub fn is_storage_controller(&self) -> bool {
        matches!(self.class_code.class, 0x01)
    }

    pub fn is_network_controller(&self) -> bool {
        matches!(self.class_code.class, 0x02)
    }

    pub fn is_gpu_controller(&self) -> bool {
        matches!(self.class_code.class, 0x03)
    }

    pub fn is_audio_controller(&self) -> bool {
        matches!((self.class_code.class, self.class_code.subclass), (0x04, 0x01))
    }

    pub fn is_usb_controller(&self) -> bool {
        matches!((self.class_code.class, self.class_code.subclass), (0x0C, 0x03))
    }

    pub fn is_virtio_device(&self) -> bool {
        self.virtio_transport.is_some()
    }

    pub fn is_virtio_gpu(&self) -> bool {
        matches!(self.virtio_device_type, Some(VirtioDeviceType::Gpu))
    }

    pub fn gpu_score(&self) -> u8 {
        match (self.is_gpu_controller(), self.is_virtio_gpu()) {
            (true, true) => 3,
            (true, false) => 2,
            (false, true) => 1,
            _ => 0,
        }
    }

    pub fn multifunction(&self) -> bool {
        self.header_type & 0x80 != 0
    }

    pub fn normalized_header_type(&self) -> u8 {
        self.header_type & PCI_HEADER_TYPE_MASK
    }

    pub fn capabilities(&self) -> &[Option<PciCapability>] {
        &self.capabilities[..self.capability_count]
    }

    pub fn primary_bar(&self) -> Option<PciBar> {
        self.bars.iter().flatten().copied().find(|bar| bar.base != 0)
    }

    pub fn summary_line(&self) -> String {
        let mut out = String::new();
        let _ = write!(&mut out, "{self}");
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct PciInventory {
    devices: Vec<PciDevice>,
}

impl PciInventory {
    pub fn scan() -> Self {
        let mut inventory = Self::default();

        for bus in 0u16..=255 {
            for device in 0u8..32 {
                let address = PciAddress {
                    segment: 0,
                    bus: bus as u8,
                    device,
                    function: 0,
                };

                if let Some(header) = read_device(address) {
                    let multifunction = header.multifunction();
                    inventory.devices.push(header);

                    if multifunction {
                        for function in 1u8..8 {
                            if let Some(device) = read_device(PciAddress {
                                segment: 0,
                                bus: bus as u8,
                                device,
                                function,
                            }) {
                                inventory.devices.push(device);
                            }
                        }
                    }
                }
            }
        }

        inventory
    }

    pub fn devices(&self) -> &[PciDevice] {
        &self.devices
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn devices_by_class(&self, class: u8) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|device| device.class_code.class == class)
            .collect()
    }

    pub fn gpu_devices(&self) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|device| device.gpu_score() > 0)
            .collect()
    }

    pub fn primary_gpu(&self) -> Option<&PciDevice> {
        self.devices.iter().max_by_key(|device| device.gpu_score())
    }

    pub fn device_lines(&self) -> Vec<String> {
        self.devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let mut line = String::new();
                let _ = write!(&mut line, "#{index:03} {device}");
                line
            })
            .collect()
    }

    pub fn inventory_summary(&self) -> String {
        let mut out = String::new();
        let gpu_count = self.gpu_devices().len();
        let network_count = self.devices.iter().filter(|device| device.is_network_controller()).count();
        let storage_count = self.devices.iter().filter(|device| device.is_storage_controller()).count();
        let usb_count = self.devices.iter().filter(|device| device.is_usb_controller()).count();
        let audio_count = self.devices.iter().filter(|device| device.is_audio_controller()).count();
        let bridge_count = self.devices.iter().filter(|device| device.is_bridge()).count();

        let _ = write!(
            &mut out,
            "pci inventory: total={} gpu={} net={} storage={} usb={} audio={} bridge={}",
            self.len(),
            gpu_count,
            network_count,
            storage_count,
            usb_count,
            audio_count,
            bridge_count
        );
        out
    }

    pub fn gpu_summary(&self) -> String {
        let mut out = String::new();
        if let Some(device) = self.primary_gpu() {
            let _ = write!(&mut out, "primary gpu: {device}");
        } else {
            let _ = write!(&mut out, "primary gpu: none");
        }
        out
    }
}

fn read_device(address: PciAddress) -> Option<PciDevice> {
    let vendor_device = read_config_u32(address, 0x00);
    let vendor_id = (vendor_device & 0xFFFF) as u16;
    if vendor_id == 0xFFFF {
        return None;
    }

    let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
    let class_info = read_config_u32(address, 0x08);
    let header_info = read_config_u32(address, 0x0C);
    let status_command = read_config_u32(address, 0x04);
    let status = ((status_command >> 16) & 0xFFFF) as u16;
    let header_type = ((header_info >> 16) & 0xFF) as u8;
    let normalized_header = header_type & PCI_HEADER_TYPE_MASK;

    let bars = read_bars(address, normalized_header);
    let (capabilities, capability_count, msi_capable, msix_capable, has_vendor_cap) =
        if status & STATUS_CAPABILITIES_LIST != 0 {
            read_capabilities(address)
        } else {
            ([None; 8], 0, false, false, false)
        };

    let virtio_transport = detect_virtio_transport(vendor_id, device_id, has_vendor_cap);
    let virtio_device_type = virtio_transport.map(|_| virtio_device_type(device_id));

    Some(PciDevice {
        address,
        vendor_id: PciVendorId(vendor_id),
        device_id: PciDeviceId(device_id),
        revision: (class_info & 0xFF) as u8,
        class_code: PciClassCode {
            programming_interface: ((class_info >> 8) & 0xFF) as u8,
            subclass: ((class_info >> 16) & 0xFF) as u8,
            class: ((class_info >> 24) & 0xFF) as u8,
        },
        header_type,
        bars,
        capabilities,
        capability_count,
        msi_capable,
        msix_capable,
        virtio_transport,
        virtio_device_type,
    })
}

fn read_bars(address: PciAddress, header_type: u8) -> [Option<PciBar>; 6] {
    let mut bars = [None; 6];
    let bar_limit = match header_type {
        0x00 => 6,
        0x01 => 2,
        _ => 0,
    };

    let mut index = 0;
    while index < bar_limit {
        let offset = 0x10 + (index as u8) * 4;
        let raw = read_config_u32(address, offset);

        if raw == 0 {
            index += 1;
            continue;
        }

        if raw & 0x1 == 0x1 {
            bars[index] = Some(PciBar {
                kind: PciBarKind::IoPort,
                base: (raw & 0xFFFF_FFFC) as u64,
                size: 0,
            });
            index += 1;
            continue;
        }

        let bar_type = (raw >> 1) & 0x3;
        if bar_type == 0x2 && index + 1 < bar_limit {
            let upper = read_config_u32(address, offset + 4) as u64;
            bars[index] = Some(PciBar {
                kind: PciBarKind::Mmio64,
                base: ((upper << 32) | ((raw & 0xFFFF_FFF0) as u64)),
                size: 0,
            });
            index += 2;
        } else {
            bars[index] = Some(PciBar {
                kind: PciBarKind::Mmio32,
                base: (raw & 0xFFFF_FFF0) as u64,
                size: 0,
            });
            index += 1;
        }
    }

    bars
}

fn read_capabilities(address: PciAddress) -> ([Option<PciCapability>; 8], usize, bool, bool, bool) {
    let mut capabilities = [None; 8];
    let mut count = 0usize;
    let mut seen = 0u16;
    let mut offset = read_config_u8(address, 0x34) & !0x3;
    let mut msi_capable = false;
    let mut msix_capable = false;
    let mut vendor_cap = false;

    while offset >= 0x40 && count < capabilities.len() && seen < 32 {
        let header = read_config_u32(address, offset);
        let id = (header & 0xFF) as u8;
        let next = ((header >> 8) & 0xFF) as u8 & !0x3;

        let capability_id = match id {
            PCI_CAP_ID_MSI => {
                msi_capable = true;
                PciCapabilityId::Msi
            }
            PCI_CAP_ID_MSIX => {
                msix_capable = true;
                PciCapabilityId::Msix
            }
            PCI_CAP_ID_VENDOR => {
                vendor_cap = true;
                PciCapabilityId::VendorSpecific
            }
            other => PciCapabilityId::Other(other),
        };

        capabilities[count] = Some(PciCapability {
            offset,
            id: capability_id,
        });
        count += 1;

        if next == offset {
            break;
        }

        offset = next;
        seen += 1;
    }

    (capabilities, count, msi_capable, msix_capable, vendor_cap)
}

fn read_config_u8(address: PciAddress, offset: u8) -> u8 {
    let value = read_config_u32(address, offset);
    let shift = (offset & 0x3) * 8;
    ((value >> shift) & 0xFF) as u8
}

/// Writes a 32-bit PCI configuration word.
pub fn write_config_u32(address: PciAddress, offset: u8, value: u32) {
    let config_address = 0x8000_0000u32
        | ((address.bus as u32) << 16)
        | ((address.device as u32) << 11)
        | ((address.function as u32) << 8)
        | ((offset as u32) & 0xFC);

    let mut addr_port = Port::<u32>::new(CONFIG_ADDRESS_PORT);
    let mut data_port = Port::<u32>::new(CONFIG_DATA_PORT);

    unsafe {
        addr_port.write(config_address);
        data_port.write(value);
    }
}

/// Enables bus mastering + memory/IO access for a device.
pub fn enable_bus_master(address: PciAddress) {
    let command = read_config_u32(address, 0x04) & 0xFFFF;
    write_config_u32(address, 0x04, command | 0x7);
}

fn read_config_u32(address: PciAddress, offset: u8) -> u32 {
    let config_address = 0x8000_0000u32
        | ((address.bus as u32) << 16)
        | ((address.device as u32) << 11)
        | ((address.function as u32) << 8)
        | ((offset as u32) & 0xFC);

    let mut addr_port = Port::<u32>::new(CONFIG_ADDRESS_PORT);
    let mut data_port = Port::<u32>::new(CONFIG_DATA_PORT);

    unsafe {
        addr_port.write(config_address);
        data_port.read()
    }
}

fn detect_virtio_transport(
    vendor_id: u16,
    device_id: u16,
    has_vendor_cap: bool,
) -> Option<VirtioTransport> {
    if vendor_id != 0x1AF4 {
        return None;
    }

    if (0x1000..=0x103F).contains(&device_id) {
        Some(VirtioTransport::Modern)
    } else if (0x100..=0x13F).contains(&device_id) || has_vendor_cap {
        Some(VirtioTransport::Legacy)
    } else {
        Some(VirtioTransport::Legacy)
    }
}

fn virtio_device_type(device_id: u16) -> VirtioDeviceType {
    match device_id {
        0x1000 | 0x1041 => VirtioDeviceType::Network,
        0x1001 | 0x1042 => VirtioDeviceType::Block,
        0x1003 | 0x1043 => VirtioDeviceType::Console,
        0x1005 | 0x1044 => VirtioDeviceType::Entropy,
        0x1002 | 0x1045 => VirtioDeviceType::Balloon,
        0x1004 | 0x1048 => VirtioDeviceType::ScsiHost,
        0x1050 => VirtioDeviceType::Gpu,
        0x1052 => VirtioDeviceType::Input,
        0x1053 => VirtioDeviceType::Socket,
        0x1049 => VirtioDeviceType::Sound,
        other => VirtioDeviceType::Unknown(other),
    }
}

impl fmt::Display for PciDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}.{} vendor={:04x} device={:04x} class={} ({:02x}:{:02x})",
            self.address.bus,
            self.address.device,
            self.address.function,
            self.vendor_id.0,
            self.device_id.0,
            self.class_name(),
            self.class_code.class,
            self.class_code.subclass
        )?;

        if self.msi_capable {
            write!(f, " msi")?;
        }
        if self.msix_capable {
            write!(f, " msix")?;
        }
        if let Some(transport) = self.virtio_transport {
            write!(f, " virtio={transport:?}")?;
        }
        if let Some(bar) = self.primary_bar() {
            write!(f, " bar0={:?}@0x{:x}", bar.kind, bar.base)?;
        }
        Ok(())
    }
}
