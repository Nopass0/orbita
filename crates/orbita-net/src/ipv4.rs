//! IPv4 layer: addresses, header parse/build, header checksum.

use orbita_std::{String, format};

/// An IPv4 address.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Ipv4Address(pub [u8; 4]);

impl Ipv4Address {
    pub const LOCALHOST: Ipv4Address = Ipv4Address([127, 0, 0, 1]);
    pub const BROADCAST: Ipv4Address = Ipv4Address([255, 255, 255, 255]);
    pub const ZERO: Ipv4Address = Ipv4Address([0, 0, 0, 0]);

    pub const fn new(octets: [u8; 4]) -> Self {
        Self(octets)
    }

    /// Dotted-decimal text form.
    pub fn text(&self) -> String {
        let [a, b, c, d] = self.0;
        format!("{a}.{b}.{c}.{d}")
    }

    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }

    pub fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] & 0xF0 == 0xE0
    }

    /// True when `other` is inside the `/prefix` network of this address.
    pub fn same_network(&self, other: Ipv4Address, prefix: u8) -> bool {
        if prefix == 0 {
            return true;
        }
        if prefix > 32 {
            return false;
        }
        let bits = u32::from_be_bytes(self.0);
        let other_bits = u32::from_be_bytes(other.0);
        let mask = if prefix == 32 {
            u32::MAX
        } else {
            (!0u32) << (32 - prefix as u32)
        };
        bits & mask == other_bits & mask
    }
}

/// Well-known IP protocol numbers used by the stack.
pub mod protocol {
    pub const ICMP: u8 = 1;
    pub const TCP: u8 = 6;
    pub const UDP: u8 = 17;
}

/// A parsed IPv4 header.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Ipv4Header {
    pub dscp_ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub checksum: u16,
    pub source: Ipv4Address,
    pub destination: Ipv4Address,
    /// Header length in bytes (IHL included).
    pub header_len: usize,
}

impl Ipv4Header {
    /// Parses a header, verifying version, IHL bounds, and checksum.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }
        let version = data[0] >> 4;
        if version != 4 {
            return None;
        }
        let ihl = (data[0] & 0x0F) as usize;
        if ihl < 5 {
            return None;
        }
        let header_len = ihl * 4;
        if data.len() < header_len {
            return None;
        }
        let header = &data[..header_len];
        if internet_checksum(header) != 0 {
            return None;
        }
        Some(Self {
            dscp_ecn: header[1],
            total_length: u16::from_be_bytes([header[2], header[3]]),
            identification: u16::from_be_bytes([header[4], header[5]]),
            flags_fragment: u16::from_be_bytes([header[6], header[7]]),
            ttl: header[8],
            protocol: header[9],
            checksum: u16::from_be_bytes([header[10], header[11]]),
            source: Ipv4Address::new([header[12], header[13], header[14], header[15]]),
            destination: Ipv4Address::new([header[16], header[17], header[18], header[19]]),
            header_len,
        })
    }

    /// Builds a 20-byte header into `out` for `payload_len` bytes of
    /// payload; the checksum is computed automatically, DF is set.
    pub fn build(
        source: Ipv4Address,
        destination: Ipv4Address,
        protocol: u8,
        payload_len: usize,
        identification: u16,
        ttl: u8,
        out: &mut [u8],
    ) -> Option<usize> {
        const LEN: usize = 20;
        if out.len() < LEN {
            return None;
        }
        let total = (LEN + payload_len) as u16;
        out[0] = 0x45; // version 4, IHL 5
        out[1] = 0;
        out[2..4].copy_from_slice(&total.to_be_bytes());
        out[4..6].copy_from_slice(&identification.to_be_bytes());
        out[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // DF
        out[8] = ttl;
        out[9] = protocol;
        out[10..12].copy_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        out[12..16].copy_from_slice(&source.0);
        out[16..20].copy_from_slice(&destination.0);
        let checksum = internet_checksum(&out[..LEN]);
        out[10..12].copy_from_slice(&checksum.to_be_bytes());
        Some(LEN)
    }

    /// Payload slice following this header inside `data`.
    pub fn payload<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        &data[self.header_len.min(data.len())..]
    }
}

/// RFC 1071 internet checksum (one's complement sum). Returns the value to
/// store; verifying a header with its checksum included yields 0.
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for chunk in &mut chunks {
        sum = sum.wrapping_add(u16::from_be_bytes([chunk[0], chunk[1]]) as u32);
    }
    if let &[byte] = chunks.remainder() {
        // odd tail: pad with a zero byte
        sum = sum.wrapping_add((byte as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_text_and_classes() {
        assert_eq!(Ipv4Address::LOCALHOST.text(), "127.0.0.1");
        assert!(Ipv4Address::LOCALHOST.is_loopback());
        assert!(Ipv4Address::new([224, 0, 0, 1]).is_multicast());
        assert!(Ipv4Address::BROADCAST.is_broadcast());
    }

    #[test]
    fn same_network_cidr() {
        let a = Ipv4Address::new([192, 168, 1, 10]);
        let b = Ipv4Address::new([192, 168, 1, 200]);
        let c = Ipv4Address::new([192, 168, 2, 10]);
        assert!(a.same_network(b, 24));
        assert!(!a.same_network(c, 24));
        assert!(a.same_network(c, 16));
    }

    #[test]
    fn header_build_parse_roundtrip() {
        let mut buf = [0u8; 20];
        let n = Ipv4Header::build(
            Ipv4Address::new([10, 0, 2, 15]),
            Ipv4Address::new([10, 0, 2, 2]),
            protocol::UDP,
            100,
            0x1234,
            64,
            &mut buf,
        )
        .unwrap();
        assert_eq!(n, 20);
        let header = Ipv4Header::parse(&buf).expect("checksum valid");
        assert_eq!(header.source, Ipv4Address::new([10, 0, 2, 15]));
        assert_eq!(header.destination, Ipv4Address::new([10, 0, 2, 2]));
        assert_eq!(header.protocol, protocol::UDP);
        assert_eq!(header.total_length, 120);
        assert_eq!(header.ttl, 64);
    }

    #[test]
    fn checksum_detects_corruption() {
        let mut buf = [0u8; 20];
        Ipv4Header::build(
            Ipv4Address::ZERO,
            Ipv4Address::BROADCAST,
            protocol::ICMP,
            8,
            1,
            64,
            &mut buf,
        )
        .unwrap();
        buf[19] ^= 0x55;
        assert!(Ipv4Header::parse(&buf).is_none());
    }
}
