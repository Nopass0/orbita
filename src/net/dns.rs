//! Domain Name System (DNS) client
use alloc::string::String;
use alloc::vec::Vec;

/// DNS query representation
pub struct DnsQuery {
    pub name: String,
    pub qtype: u16,
    pub qclass: u16,
}

impl DnsQuery {
    /// Serialize DNS query
    pub fn serialize(&self, out: &mut Vec<u8>) {
        for part in self.name.split('.') {
            out.push(part.len() as u8);
            out.extend_from_slice(part.as_bytes());
        }
        out.push(0); // end of name
        out.extend_from_slice(&self.qtype.to_be_bytes());
        out.extend_from_slice(&self.qclass.to_be_bytes());
    }
}
