//! User Datagram Protocol (UDP)
use alloc::vec::Vec;

/// UDP packet structure
pub struct UdpPacket<'a> {
    pub source_port: u16,
    pub dest_port: u16,
    pub payload: &'a [u8],
}

impl<'a> UdpPacket<'a> {
    /// Serialize UDP packet
    pub fn serialize(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.source_port.to_be_bytes());
        out.extend_from_slice(&self.dest_port.to_be_bytes());
        let len = (8 + self.payload.len()) as u16;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        out.extend_from_slice(self.payload);
    }
}
