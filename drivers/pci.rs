#![no_std]

//! Simple PCI bus scanning utilities

use alloc::vec::Vec;
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// PCI device identifier
#[derive(Debug, Clone, Copy)]
pub struct PciDeviceId {
    pub vendor_id: u16,
    pub device_id: u16,
}

/// Basic PCI device information
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub id: PciDeviceId,
    pub class: u8,
    pub subclass: u8,
    pub bar0: u32,
}

fn read_config_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
        | 0x8000_0000;
    unsafe {
        let mut addr = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data = Port::<u32>::new(CONFIG_DATA);
        addr.write(address);
        data.read()
    }
}

fn read_config_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let dword = read_config_dword(bus, device, function, offset);
    ((dword >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

fn read_config_byte(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    (read_config_word(bus, device, function, offset & 0xFE) >> ((offset & 1) * 8)) as u8
}

fn device_exists(bus: u8, device: u8, function: u8) -> bool {
    read_config_word(bus, device, function, 0x00) != 0xFFFF
}

fn read_device(bus: u8, device: u8, function: u8) -> PciDevice {
    let vendor = read_config_word(bus, device, function, 0x00);
    let device_id = read_config_word(bus, device, function, 0x02);
    let class_info = read_config_dword(bus, device, function, 0x08);
    let class = (class_info >> 24) as u8;
    let subclass = (class_info >> 16) as u8;
    let bar0 = read_config_dword(bus, device, function, 0x10);
    PciDevice {
        bus,
        device,
        function,
        id: PciDeviceId { vendor_id: vendor, device_id },
        class,
        subclass,
        bar0,
    }
}

/// Scan the entire PCI bus
pub fn scan_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0u8..=255 {
        for dev in 0u8..32 {
            for func in 0u8..8 {
                if !device_exists(bus, dev, func) {
                    if func == 0 {
                        break;
                    }
                    continue;
                }
                devices.push(read_device(bus, dev, func));
            }
        }
    }
    devices
}

/// Find all devices with the given class and subclass
pub fn find_by_class(class: u8, subclass: u8) -> Vec<PciDevice> {
    scan_bus()
        .into_iter()
        .filter(|d| d.class == class && d.subclass == subclass)
        .collect()
}

/// Find all audio devices (class code 0x04)
pub fn find_audio_devices() -> Vec<PciDevice> {
    scan_bus().into_iter().filter(|d| d.class == 0x04).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id() {
        let id = PciDeviceId {
            vendor_id: 0x8086,
            device_id: 0x1234,
        };
        assert_eq!(id.vendor_id, 0x8086);
        assert_eq!(id.device_id, 0x1234);
    }
}
