//! TCP: segment parse/build with IPv4 pseudo-header checksums and flag
//! constants. Connection state management is intentionally out of scope
//! here (owned by the future socket layer).

use crate::ipv4::{protocol, Ipv4Address};

/// A parsed TCP segment (header + payload view).
#[derive(Debug, Clone)]
pub struct TcpSegment<'a> {
    pub source_port: u16,
    pub destination_port: u16,
    pub sequence: u32,
    pub acknowledgment: u32,
    pub data_offset: usize,
    pub flags: TcpFlags,
    pub window: u16,
    pub checksum: u16,
    pub payload: &'a [u8],
}

/// TCP header flags.
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
pub struct TcpFlags(pub u8);

impl TcpFlags {
    pub const FIN: u8 = 0x01;
    pub const SYN: u8 = 0x02;
    pub const RST: u8 = 0x04;
    pub const PSH: u8 = 0x08;
    pub const ACK: u8 = 0x10;
    pub const URG: u8 = 0x20;

    pub fn set(mut self, flag: u8) -> Self {
        self.0 |= flag;
        self
    }

    pub fn has(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    pub fn text(self) -> orbita_std::String {
        let mut out = orbita_std::String::new();
        for (bit, name) in [
            (Self::FIN, "FIN"),
            (Self::SYN, "SYN"),
            (Self::RST, "RST"),
            (Self::PSH, "PSH"),
            (Self::ACK, "ACK"),
            (Self::URG, "URG"),
        ] {
            if self.has(bit) {
                if !out.is_empty() {
                    out.push('+');
                }
                out.push_str(name);
            }
        }
        out
    }
}

/// Minimum TCP header length.
pub const HEADER_LEN: usize = 20;

impl<'a> TcpSegment<'a> {
    /// Parses a segment, verifying the checksum.
    pub fn parse(data: &'a [u8], source: Ipv4Address, destination: Ipv4Address) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }
        let data_offset = ((data[12] >> 4) as usize) * 4;
        if data_offset < HEADER_LEN || data.len() < data_offset {
            return None;
        }
        let segment = Self {
            source_port: u16::from_be_bytes([data[0], data[1]]),
            destination_port: u16::from_be_bytes([data[2], data[3]]),
            sequence: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            acknowledgment: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            data_offset,
            flags: TcpFlags(data[13]),
            window: u16::from_be_bytes([data[14], data[15]]),
            checksum: u16::from_be_bytes([data[16], data[17]]),
            payload: &data[data_offset..],
        };
        if tcp_checksum(source, destination, data) != 0 {
            return None;
        }
        Some(segment)
    }

    /// Builds a segment into `out`, checksum included.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        source: Ipv4Address,
        source_port: u16,
        destination: Ipv4Address,
        destination_port: u16,
        sequence: u32,
        acknowledgment: u32,
        flags: TcpFlags,
        window: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let total = HEADER_LEN + payload.len();
        if out.len() < total {
            return None;
        }
        out[0..2].copy_from_slice(&source_port.to_be_bytes());
        out[2..4].copy_from_slice(&destination_port.to_be_bytes());
        out[4..8].copy_from_slice(&sequence.to_be_bytes());
        out[8..12].copy_from_slice(&acknowledgment.to_be_bytes());
        out[12] = ((HEADER_LEN / 4) as u8) << 4; // data offset, no options
        out[13] = flags.0;
        out[14..16].copy_from_slice(&window.to_be_bytes());
        out[16..18].copy_from_slice(&0u16.to_be_bytes()); // checksum
        out[18..20].copy_from_slice(&0u16.to_be_bytes()); // urgent pointer
        out[HEADER_LEN..total].copy_from_slice(payload);
        let checksum = tcp_checksum(source, destination, &out[..total]);
        out[16..18].copy_from_slice(&checksum.to_be_bytes());
        Some(total)
    }
}

/// Valid segment sums to 0.
fn tcp_checksum(source: Ipv4Address, destination: Ipv4Address, segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    let mut add_u16 = |word: u16| sum = sum.wrapping_add(word as u32);
    for &octet in &source.0 {
        add_u16((octet as u16) << 8);
    }
    for &octet in &destination.0 {
        add_u16(octet as u16);
    }
    add_u16(protocol::TCP as u16);
    add_u16(segment.len() as u16);

    let mut chunks = segment.chunks_exact(2);
    for chunk in &mut chunks {
        add_u16(u16::from_be_bytes([chunk[0], chunk[1]]));
    }
    if let &[byte] = chunks.remainder() {
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
    fn flags_text() {
        let flags = TcpFlags::default().set(TcpFlags::SYN);
        assert_eq!(flags.text(), "SYN");
        let flags = TcpFlags::default().set(TcpFlags::SYN).set(TcpFlags::ACK);
        assert_eq!(flags.text(), "SYN+ACK");
    }

    #[test]
    fn syn_segment_roundtrip() {
        let mut buf = [0u8; HEADER_LEN + 8];
        let n = TcpSegment::build(
            Ipv4Address::new([10, 0, 2, 15]),
            49152,
            Ipv4Address::new([93, 184, 216, 34]),
            443,
            1000,
            0,
            TcpFlags::default().set(TcpFlags::SYN),
            65535,
            b"",
            &mut buf,
        )
        .unwrap();
        assert_eq!(n, HEADER_LEN);
        let seg = TcpSegment::parse(
            &buf[..n],
            Ipv4Address::new([10, 0, 2, 15]),
            Ipv4Address::new([93, 184, 216, 34]),
        )
        .expect("checksum valid");
        assert_eq!(seg.source_port, 49152);
        assert_eq!(seg.destination_port, 443);
        assert_eq!(seg.sequence, 1000);
        assert!(seg.flags.has(TcpFlags::SYN));
        assert!(seg.payload.is_empty());
    }

    #[test]
    fn data_segment_roundtrip() {
        let payload = b"GET / HTTP/1.1";
        let mut buf = [0u8; HEADER_LEN + 32];
        let n = TcpSegment::build(
            Ipv4Address::LOCALHOST,
            1,
            Ipv4Address::LOCALHOST,
            80,
            5,
            2000,
            TcpFlags::default().set(TcpFlags::ACK).set(TcpFlags::PSH),
            8192,
            payload,
            &mut buf,
        )
        .unwrap();
        let seg = TcpSegment::parse(&buf[..n], Ipv4Address::LOCALHOST, Ipv4Address::LOCALHOST)
            .unwrap();
        assert_eq!(seg.payload, &payload[..]);
        assert_eq!(seg.window, 8192);
    }
}
