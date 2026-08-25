//! ARP: address resolution for IPv4 over Ethernet.

use crate::ethernet::MacAddress;
use orbita_std::Vec;

/// ARP operation.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ArpOperation {
    Request,
    Reply,
}

impl ArpOperation {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(ArpOperation::Request),
            2 => Some(ArpOperation::Reply),
            _ => None,
        }
    }

    pub fn to_u16(self) -> u16 {
        match self {
            ArpOperation::Request => 1,
            ArpOperation::Reply => 2,
        }
    }
}

/// A parsed ARP packet (Ethernet/IPv4 hardware/protocol only).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ArpPacket {
    pub operation: ArpOperation,
    pub sender_mac: MacAddress,
    pub sender_ip: [u8; 4],
    pub target_mac: MacAddress,
    pub target_ip: [u8; 4],
}

/// ARP packet length for Ethernet+IPv4.
pub const PACKET_LEN: usize = 28;

impl ArpPacket {
    /// Parses an ARP packet.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < PACKET_LEN {
            return None;
        }
        // htype=1 (ethernet), ptype=0x0800 (ipv4), hlen=6, plen=4
        if data[0] != 0x00 || data[1] != 0x01 || data[2] != 0x08 || data[3] != 0x00 {
            return None;
        }
        if data[4] != 6 || data[5] != 4 {
            return None;
        }
        Some(Self {
            operation: ArpOperation::from_u16(u16::from_be_bytes([data[6], data[7]]))?,
            sender_mac: MacAddress::new([data[8], data[9], data[10], data[11], data[12], data[13]]),
            sender_ip: [data[14], data[15], data[16], data[17]],
            target_mac: MacAddress::new([data[18], data[19], data[20], data[21], data[22], data[23]]),
            target_ip: [data[24], data[25], data[26], data[27]],
        })
    }

    /// Serializes into `out` (28 bytes needed).
    pub fn build(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < PACKET_LEN {
            return None;
        }
        out[0..2].copy_from_slice(&[0x00, 0x01]);
        out[2..4].copy_from_slice(&[0x08, 0x00]);
        out[4] = 6;
        out[5] = 4;
        out[6..8].copy_from_slice(&self.operation.to_u16().to_be_bytes());
        out[8..14].copy_from_slice(&self.sender_mac.0);
        out[14..18].copy_from_slice(&self.sender_ip);
        out[18..24].copy_from_slice(&self.target_mac.0);
        out[24..28].copy_from_slice(&self.target_ip);
        Some(PACKET_LEN)
    }

    /// Builds the reply answering a request: swaps the peers and fills the
    /// target MAC with the requester's own MAC.
    pub fn make_reply(&self, our_mac: MacAddress, our_ip: [u8; 4]) -> Self {
        Self {
            operation: ArpOperation::Reply,
            sender_mac: our_mac,
            sender_ip: our_ip,
            target_mac: self.sender_mac,
            target_ip: self.sender_ip,
        }
    }
}

/// A simple ARP cache: IP → MAC bindings learned from traffic.
#[derive(Debug, Default)]
pub struct ArpCache {
    entries: Vec<([u8; 4], MacAddress)>,
}

impl ArpCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, ip: [u8; 4], mac: MacAddress) {
        for entry in &mut self.entries {
            if entry.0 == ip {
                entry.1 = mac;
                return;
            }
        }
        self.entries.push((ip, mac));
    }

    pub fn lookup(&self, ip: [u8; 4]) -> Option<MacAddress> {
        self.entries.iter().find(|e| e.0 == ip).map(|e| e.1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arp_roundtrip_and_reply() {
        let req = ArpPacket {
            operation: ArpOperation::Request,
            sender_mac: MacAddress::new([1, 1, 1, 1, 1, 1]),
            sender_ip: [10, 0, 2, 2],
            target_mac: MacAddress::ZERO,
            target_ip: [10, 0, 2, 15],
        };
        let mut buf = [0u8; PACKET_LEN];
        let n = req.build(&mut buf).unwrap();
        assert_eq!(n, PACKET_LEN);
        let parsed = ArpPacket::parse(&buf).unwrap();
        assert_eq!(parsed, req);

        let reply = parsed.make_reply(MacAddress::new([2, 2, 2, 2, 2, 2]), [10, 0, 2, 15]);
        assert_eq!(reply.operation, ArpOperation::Reply);
        assert_eq!(reply.target_ip, [10, 0, 2, 2]);
        assert_eq!(reply.sender_ip, [10, 0, 2, 15]);
    }

    #[test]
    fn cache_learn_and_lookup() {
        let mut cache = ArpCache::new();
        cache.insert([10, 0, 2, 2], MacAddress::new([1, 1, 1, 1, 1, 1]));
        assert_eq!(cache.lookup([10, 0, 2, 2]).unwrap().0, [1, 1, 1, 1, 1, 1]);
        assert!(cache.lookup([10, 0, 2, 3]).is_none());
    }
}
