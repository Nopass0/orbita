//! Transmission Control Protocol (TCP)
use alloc::vec::Vec;

/// TCP header flags
pub struct TcpFlags;

/// TCP packet structure
pub struct TcpPacket<'a> {
    pub source_port: u16,
    pub dest_port: u16,
    pub seq_number: u32,
    pub ack_number: u32,
    pub flags: u16,
    pub window_size: u16,
    pub payload: &'a [u8],
}

impl<'a> TcpPacket<'a> {
    /// Serialize TCP packet (without options)
    pub fn serialize(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.source_port.to_be_bytes());
        out.extend_from_slice(&self.dest_port.to_be_bytes());
        out.extend_from_slice(&self.seq_number.to_be_bytes());
        out.extend_from_slice(&self.ack_number.to_be_bytes());
        let data_offset = 5u8 << 4; // no options
        out.push(data_offset);
        out.push((self.flags & 0xff) as u8);
        out.extend_from_slice(&self.window_size.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        out.extend_from_slice(&0u16.to_be_bytes()); // urgent pointer
        out.extend_from_slice(self.payload);
    }
}
