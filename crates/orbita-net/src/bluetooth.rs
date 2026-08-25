//! Bluetooth device model: addresses, device classes, pairing state.
//!
//! Like Wi-Fi this is the data/contract layer; HCI transport belongs to
//! the driver backend.

use orbita_std::{String, Vec, format};

/// A 48-bit Bluetooth address (displayed most-significant first).
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct BluetoothAddress(pub [u8; 6]);

impl BluetoothAddress {
    pub fn text(&self) -> String {
        let [a, b, c, d, e, f] = self.0;
        format!("{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{f:02X}")
    }
}

/// Coarse Bluetooth device class (major service groups).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BluetoothClass {
    Computer,
    Phone,
    Audio,
    Peripheral,
    Wearable,
    Unknown,
}

impl BluetoothClass {
    pub fn label(self) -> &'static str {
        match self {
            BluetoothClass::Computer => "computer",
            BluetoothClass::Phone => "phone",
            BluetoothClass::Audio => "audio",
            BluetoothClass::Peripheral => "peripheral",
            BluetoothClass::Wearable => "wearable",
            BluetoothClass::Unknown => "unknown",
        }
    }
}

/// Pairing state with a remote device.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PairingState {
    NotPaired,
    Pairing,
    Paired,
}

/// A remote Bluetooth device.
#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    pub address: BluetoothAddress,
    pub name: String,
    pub class: BluetoothClass,
    pub state: PairingState,
}

impl BluetoothDevice {
    pub fn summary(&self) -> String {
        format!(
            "{} [{}] {} {}",
            self.name,
            self.class.label(),
            self.address.text(),
            match self.state {
                PairingState::NotPaired => "not-paired",
                PairingState::Pairing => "pairing",
                PairingState::Paired => "paired",
            }
        )
    }
}

/// Local Bluetooth controller and its known remotes.
#[derive(Debug)]
pub struct BluetoothHost {
    pub address: BluetoothAddress,
    pub discoverable: bool,
    pub devices: Vec<BluetoothDevice>,
}

impl BluetoothHost {
    pub fn new(address: BluetoothAddress) -> Self {
        Self {
            address,
            discoverable: false,
            devices: Vec::new(),
        }
    }

    /// Adds or refreshes a discovered remote.
    pub fn discover(&mut self, address: BluetoothAddress, name: &str, class: BluetoothClass) {
        if let Some(existing) = self
            .devices
            .iter_mut()
            .find(|d| d.address == address)
        {
            existing.name = String::from(name);
            existing.class = class;
            return;
        }
        self.devices.push(BluetoothDevice {
            address,
            name: String::from(name),
            class,
            state: PairingState::NotPaired,
        });
    }

    /// Marks a remote as paired; returns false when unknown.
    pub fn pair(&mut self, address: BluetoothAddress) -> bool {
        match self.devices.iter_mut().find(|d| d.address == address) {
            Some(device) => {
                device.state = PairingState::Paired;
                true
            }
            None => false,
        }
    }

    pub fn paired_count(&self) -> usize {
        self.devices.iter().filter(|d| d.state == PairingState::Paired).count()
    }

    /// Summary line, e.g. `bt: 12:34:56:78:9A:BC devices=2 paired=1`.
    pub fn summary(&self) -> String {
        format!(
            "bt: {} devices={} paired={}",
            self.address.text(),
            self.devices.len(),
            self.paired_count()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_text() {
        let addr = BluetoothAddress([0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC]);
        assert_eq!(addr.text(), "12:34:56:78:9A:BC");
    }

    #[test]
    fn discover_and_pair() {
        let mut host = BluetoothHost::new(BluetoothAddress([0; 6]));
        host.discover(BluetoothAddress([1, 2, 3, 4, 5, 6]), "Orbita Mouse", BluetoothClass::Peripheral);
        // refresh same address
        host.discover(BluetoothAddress([1, 2, 3, 4, 5, 6]), "Orbita Mouse 2", BluetoothClass::Peripheral);
        host.discover(BluetoothAddress([9, 9, 9, 9, 9, 9]), "Speakers", BluetoothClass::Audio);
        assert_eq!(host.devices.len(), 2);
        assert!(host.pair(BluetoothAddress([1, 2, 3, 4, 5, 6])));
        assert!(!host.pair(BluetoothAddress([7, 7, 7, 7, 7, 7])));
        assert_eq!(host.paired_count(), 1);
        assert!(host.summary().contains("paired=1"));
        assert!(host.devices[0].summary().contains("peripheral"));
    }
}
