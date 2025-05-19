//! Internet Control Message Protocol (ICMP)

/// ICMP packet types
pub enum IcmpType {
    EchoReply = 0,
    EchoRequest = 8,
}

/// ICMP packet structure
pub struct IcmpPacket<'a> {
    pub icmp_type: IcmpType,
    pub code: u8,
    pub checksum: u16,
    pub payload: &'a [u8],
}

impl<'a> IcmpPacket<'a> {
    /// Serialize ICMP packet
    pub fn serialize(&self, out: &mut alloc::vec::Vec<u8>) {
        out.push(self.icmp_type as u8);
        out.push(self.code);
        out.extend_from_slice(&self.checksum.to_be_bytes());
        out.extend_from_slice(self.payload);
    }
}
