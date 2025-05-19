//! IPv4 packet structures and routing
use alloc::vec::Vec;

/// IPv4 address
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Ipv4Addr(pub [u8; 4]);

/// IPv4 header
pub struct Ipv4Packet<'a> {
    pub source: Ipv4Addr,
    pub destination: Ipv4Addr,
    pub protocol: u8,
    pub payload: &'a [u8],
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
}

impl<'a> Ipv4Packet<'a> {
    /// Serialize IPv4 packet
    pub fn serialize(&self, out: &mut Vec<u8>) {
        let ihl = 5u8; // no options
        let version_ihl = (4 << 4) | ihl;
        out.push(version_ihl);
        out.push(self.ttl); // will set DSCP/ECN to 0
        let total_len = (ihl as usize * 4 + self.payload.len()) as u16;
        out.extend_from_slice(&total_len.to_be_bytes());
        out.extend_from_slice(&self.identification.to_be_bytes());
        out.extend_from_slice(&self.flags_fragment.to_be_bytes());
        out.push(self.ttl);
        out.push(self.protocol);
        out.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        out.extend_from_slice(&self.source.0);
        out.extend_from_slice(&self.destination.0);
        out.extend_from_slice(self.payload);
    }
}

/// Simple routing table entry
pub struct Route {
    pub network: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
}

/// Dummy routing table
pub struct RoutingTable {
    pub routes: Vec<Route>,
}

impl RoutingTable {
    /// Create empty table
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add a route
    pub fn add_route(&mut self, route: Route) {
        self.routes.push(route);
    }
}
