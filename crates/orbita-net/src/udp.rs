//! UDP: datagram parse/build with IPv4 pseudo-header checksums.

use crate::ipv4::Ipv4Address;

/// A parsed UDP datagram (header + payload view).
#[derive(Debug, Clone)]
pub struct UdpDatagram<'a> {
    pub source_port: u16,
    pub destination_port: u16,
    pub length: u16,
    pub checksum: u16,
    pub payload: &'a [u8],
}

/// UDP header length.
pub const HEADER_LEN: usize = 8;

impl<'a> UdpDatagram<'a> {
    /// Parses a datagram. When `verify_checksum` is set, validates the
    /// optional checksum (0 = not used by sender).
    pub fn parse(data: &'a [u8], source: Ipv4Address, destination: Ipv4Address, verify_checksum: bool) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }
        let datagram = Self {
            source_port: u16::from_be_bytes([data[0], data[1]]),
            destination_port: u16::from_be_bytes([data[2], data[3]]),
            length: u16::from_be_bytes([data[4], data[5]]),
            checksum: u16::from_be_bytes([data[6], data[7]]),
            payload: &data[HEADER_LEN..],
        };
        if datagram.length as usize != data.len() {
            return None;
        }
        if verify_checksum && datagram.checksum != 0 {
            if udp_checksum(source, destination, data) != 0 {
                return None;
            }
        }
        Some(datagram)
    }

    /// Builds a datagram (header + payload + checksum) into `out`.
    pub fn build(
        source: Ipv4Address,
        source_port: u16,
        destination: Ipv4Address,
        destination_port: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let total = HEADER_LEN + payload.len();
        if out.len() < total || total > u16::MAX as usize {
            return None;
        }
        out[0..2].copy_from_slice(&source_port.to_be_bytes());
        out[2..4].copy_from_slice(&destination_port.to_be_bytes());
        out[4..6].copy_from_slice(&(total as u16).to_be_bytes());
        out[6..8].copy_from_slice(&0u16.to_be_bytes());
        out[HEADER_LEN..total].copy_from_slice(payload);
        let checksum = udp_checksum(source, destination, &out[..total]);
        // 0 would mean "no checksum"; emit 0xFFFF in that corner case.
        let checksum = if checksum == 0 { 0xFFFF } else { checksum };
        out[6..8].copy_from_slice(&checksum.to_be_bytes());
        Some(total)
    }
}

/// Computes the UDP checksum over pseudo-header + datagram. A valid
/// datagram sums to 0.
///
/// The pseudo-header is fed word-by-word — no 64 KiB stack buffer.
fn udp_checksum(source: Ipv4Address, destination: Ipv4Address, datagram: &[u8]) -> u16 {
    let len = datagram.len().min(0xFFFF);
    let mut sum: u32 = 0;
    let add_word = |word: u16, sum: &mut u32| {
        *sum = sum.wrapping_add(u16::from_be_bytes(word.to_be_bytes()) as u32);
    };

    add_word(u16::from_be_bytes([source.0[0], source.0[1]]), &mut sum);
    add_word(u16::from_be_bytes([source.0[2], source.0[3]]), &mut sum);
    add_word(u16::from_be_bytes([destination.0[0], destination.0[1]]), &mut sum);
    add_word(u16::from_be_bytes([destination.0[2], destination.0[3]]), &mut sum);
    add_word(0, &mut sum);
    add_word(crate::ipv4::protocol::UDP as u16, &mut sum);
    add_word(len as u16, &mut sum);

    let mut chunks = datagram[..len].chunks_exact(2);
    for chunk in &mut chunks {
        sum = sum.wrapping_add(u16::from_be_bytes([chunk[0], chunk[1]]) as u32);
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
    fn udp_roundtrip_with_checksum() {
        let payload = b"hello orbita";
        let mut buf = [0u8; HEADER_LEN + 32];
        let n = UdpDatagram::build(
            Ipv4Address::new([10, 0, 2, 15]),
            6891,
            Ipv4Address::new([10, 0, 2, 2]),
            53,
            payload,
            &mut buf,
        )
        .unwrap();
        let dgram = UdpDatagram::parse(
            &buf[..n],
            Ipv4Address::new([10, 0, 2, 15]),
            Ipv4Address::new([10, 0, 2, 2]),
            true,
        )
        .expect("checksum valid");
        assert_eq!(dgram.source_port, 6891);
        assert_eq!(dgram.destination_port, 53);
        assert_eq!(dgram.payload, &payload[..]);
    }

    #[test]
    fn corruption_detected() {
        let payload = b"abcd";
        let mut buf = [0u8; HEADER_LEN + 8];
        let n = UdpDatagram::build(
            Ipv4Address::LOCALHOST,
            1,
            Ipv4Address::LOCALHOST,
            2,
            payload,
            &mut buf,
        )
        .unwrap();
        buf[n - 1] ^= 0x01;
        assert!(UdpDatagram::parse(
            &buf[..n],
            Ipv4Address::LOCALHOST,
            Ipv4Address::LOCALHOST,
            true
        )
        .is_none());
    }
}
