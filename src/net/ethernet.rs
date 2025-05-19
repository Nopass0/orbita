//! Ethernet frame structures and helpers
use alloc::vec::Vec;

/// Hardware MAC address
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Broadcast MAC address FF:FF:FF:FF:FF:FF
    pub const BROADCAST: Self = Self([0xff; 6]);
}

/// Ethernet frame header length
pub const HEADER_LEN: usize = 14;

/// Parsed Ethernet frame
pub struct EthernetFrame<'a> {
    pub destination: MacAddress,
    pub source: MacAddress,
    pub ethertype: u16,
    pub payload: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    /// Parse an Ethernet frame from raw bytes
    pub fn from_bytes(data: &'a [u8]) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }
        let destination = MacAddress([
            data[0], data[1], data[2], data[3], data[4], data[5],
        ]);
        let source = MacAddress([
            data[6], data[7], data[8], data[9], data[10], data[11],
        ]);
        let ethertype = u16::from_be_bytes([data[12], data[13]]);
        Some(Self {
            destination,
            source,
            ethertype,
            payload: &data[HEADER_LEN..],
        })
    }

    /// Serialize the Ethernet frame into the provided buffer
    pub fn serialize(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.destination.0);
        out.extend_from_slice(&self.source.0);
        out.extend_from_slice(&self.ethertype.to_be_bytes());
        out.extend_from_slice(self.payload);
    }
}
