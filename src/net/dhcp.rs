//! Dynamic Host Configuration Protocol (DHCP) client
use alloc::vec::Vec;

/// Simple DHCP discover packet
pub struct DhcpDiscover<'a> {
    pub transaction_id: u32,
    pub client_mac: &'a [u8; 6],
}

impl<'a> DhcpDiscover<'a> {
    /// Serialize DHCP discover packet (without options)
    pub fn serialize(&self, out: &mut Vec<u8>) {
        out.push(1); // op: BOOTREQUEST
        out.push(1); // htype: Ethernet
        out.push(6); // hlen
        out.push(0); // hops
        out.extend_from_slice(&self.transaction_id.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // secs
        out.extend_from_slice(&0u16.to_be_bytes()); // flags
        out.extend_from_slice(&[0u8; 4]); // ciaddr
        out.extend_from_slice(&[0u8; 4]); // yiaddr
        out.extend_from_slice(&[0u8; 4]); // siaddr
        out.extend_from_slice(&[0u8; 4]); // giaddr
        out.extend_from_slice(self.client_mac);
        out.extend_from_slice(&[0u8; 10]); // padding for chaddr
        out.extend_from_slice(&[0u8; 192]); // bootp legacy
        out.extend_from_slice(&[99, 130, 83, 99]); // magic cookie
        // options will be appended elsewhere
    }
}
