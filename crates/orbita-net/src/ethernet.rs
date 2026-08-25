//! Ethernet layer: MAC addresses and frame parse/build.

use orbita_std::{String, format};

/// A 48-bit MAC address.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    pub const BROADCAST: MacAddress = MacAddress([0xFF; 6]);
    pub const ZERO: MacAddress = MacAddress([0x00; 6]);

    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// `aa:bb:cc:dd:ee:ff` text form.
    pub fn text(&self) -> String {
        let [a, b, c, d, e, f] = self.0;
        format!("{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}")
    }

    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }

    /// Locally administered bit (used by virtual NICs).
    pub fn is_locally_administered(&self) -> bool {
        self.0[0] & 0x02 != 0
    }

    /// Multicast bit.
    pub fn is_multicast(&self) -> bool {
        self.0[0] & 0x01 != 0
    }
}

/// EtherType payload selector.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum EtherType {
    Ipv4,
    Arp,
    Ipv6,
    Unknown(u16),
}

impl EtherType {
    pub fn from_u16(value: u16) -> Self {
        match value {
            0x0800 => EtherType::Ipv4,
            0x0806 => EtherType::Arp,
            0x86DD => EtherType::Ipv6,
            other => EtherType::Unknown(other),
        }
    }

    pub fn to_u16(self) -> u16 {
        match self {
            EtherType::Ipv4 => 0x0800,
            EtherType::Arp => 0x0806,
            EtherType::Ipv6 => 0x86DD,
            EtherType::Unknown(other) => other,
        }
    }
}

/// A parsed Ethernet frame (header + payload view).
#[derive(Debug, Clone)]
pub struct EthernetFrame<'a> {
    pub destination: MacAddress,
    pub source: MacAddress,
    pub ether_type: EtherType,
    pub payload: &'a [u8],
}

/// Ethernet header length in bytes.
pub const HEADER_LEN: usize = 14;

impl<'a> EthernetFrame<'a> {
    /// Parses an incoming frame. Returns `None` when shorter than the
    /// header or when trailing data is absent.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }
        Some(Self {
            destination: MacAddress::new([data[0], data[1], data[2], data[3], data[4], data[5]]),
            source: MacAddress::new([data[6], data[7], data[8], data[9], data[10], data[11]]),
            ether_type: EtherType::from_u16(u16::from_be_bytes([data[12], data[13]])),
            payload: &data[HEADER_LEN..],
        })
    }

    /// Serializes a frame into `out`, returning the written byte count.
    pub fn build(
        destination: MacAddress,
        source: MacAddress,
        ether_type: EtherType,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let total = HEADER_LEN + payload.len();
        if out.len() < total {
            return None;
        }
        out[..6].copy_from_slice(&destination.0);
        out[6..12].copy_from_slice(&source.0);
        out[12..14].copy_from_slice(&ether_type.to_u16().to_be_bytes());
        out[HEADER_LEN..total].copy_from_slice(payload);
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn mac_text_and_flags() {
        let mac = MacAddress::new([0x02, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
        assert_eq!(mac.text(), "02:aa:bb:cc:dd:ee");
        assert!(mac.is_locally_administered());
        assert!(!mac.is_multicast());
        assert!(MacAddress::BROADCAST.is_broadcast());
    }

    #[test]
    fn frame_roundtrip() {
        let payload = [0xDE, 0xAD, 0xBE, 0xEF];
        let mut buf = vec![0u8; 32];
        let n = EthernetFrame::build(
            MacAddress::BROADCAST,
            MacAddress::new([1, 2, 3, 4, 5, 6]),
            EtherType::Ipv4,
            &payload,
            &mut buf,
        )
        .expect("fits");
        assert_eq!(n, HEADER_LEN + 4);
        let frame = EthernetFrame::parse(&buf[..n]).expect("parses");
        assert_eq!(frame.destination, MacAddress::BROADCAST);
        assert_eq!(frame.source, MacAddress::new([1, 2, 3, 4, 5, 6]));
        assert_eq!(frame.ether_type, EtherType::Ipv4);
        assert_eq!(frame.payload, &payload);
    }

    #[test]
    fn too_short_is_rejected() {
        assert!(EthernetFrame::parse(&[0u8; 10]).is_none());
    }
}
