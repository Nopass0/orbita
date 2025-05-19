//! Address Resolution Protocol (ARP)
use alloc::vec::Vec;
use crate::net::ethernet::MacAddress;
use crate::net::ipv4::Ipv4Addr;

/// ARP operation codes
pub enum ArpOp {
    Request = 1,
    Reply = 2,
}

/// ARP packet structure
pub struct ArpPacket {
    pub hw_type: u16,
    pub proto_type: u16,
    pub hw_len: u8,
    pub proto_len: u8,
    pub op: ArpOp,
    pub sender_mac: MacAddress,
    pub sender_ip: Ipv4Addr,
    pub target_mac: MacAddress,
    pub target_ip: Ipv4Addr,
}

impl ArpPacket {
    /// Serialize ARP packet to bytes
    pub fn serialize(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.hw_type.to_be_bytes());
        out.extend_from_slice(&self.proto_type.to_be_bytes());
        out.push(self.hw_len);
        out.push(self.proto_len);
        out.extend_from_slice(&(self.op as u16).to_be_bytes());
        out.extend_from_slice(&self.sender_mac.0);
        out.extend_from_slice(&self.sender_ip.0);
        out.extend_from_slice(&self.target_mac.0);
        out.extend_from_slice(&self.target_ip.0);
    }
}
