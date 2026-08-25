//! ICMP: echo request/reply for IPv4.

/// ICMP message types handled by the stack.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IcmpKind {
    EchoRequest,
    EchoReply,
    DestinationUnreachable,
    TimeExceeded,
    Unknown(u8),
}

impl IcmpKind {
    pub fn from_u8(value: u8) -> Self {
        match value {
            8 => IcmpKind::EchoRequest,
            0 => IcmpKind::EchoReply,
            3 => IcmpKind::DestinationUnreachable,
            11 => IcmpKind::TimeExceeded,
            other => IcmpKind::Unknown(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            IcmpKind::EchoRequest => 8,
            IcmpKind::EchoReply => 0,
            IcmpKind::DestinationUnreachable => 3,
            IcmpKind::TimeExceeded => 11,
            IcmpKind::Unknown(other) => other,
        }
    }
}

/// A parsed ICMP message (header + payload view).
#[derive(Debug, Clone)]
pub struct IcmpMessage<'a> {
    pub kind: IcmpKind,
    pub code: u8,
    pub checksum: u16,
    pub identifier: u16,
    pub sequence: u16,
    pub payload: &'a [u8],
}

/// ICMP header length for echo messages.
pub const HEADER_LEN: usize = 8;

impl<'a> IcmpMessage<'a> {
    /// Parses and verifies the checksum over the whole message.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }
        if crate::ipv4::internet_checksum(data) != 0 {
            return None;
        }
        Some(Self {
            kind: IcmpKind::from_u8(data[0]),
            code: data[1],
            checksum: u16::from_be_bytes([data[2], data[3]]),
            identifier: u16::from_be_bytes([data[4], data[5]]),
            sequence: u16::from_be_bytes([data[6], data[7]]),
            payload: &data[HEADER_LEN..],
        })
    }

    /// Builds an echo request/reply into `out`, checksum included.
    pub fn build_echo(
        kind: IcmpKind,
        identifier: u16,
        sequence: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let total = HEADER_LEN + payload.len();
        if out.len() < total {
            return None;
        }
        out[0] = kind.to_u8();
        out[1] = 0;
        out[2..4].copy_from_slice(&0u16.to_be_bytes());
        out[4..6].copy_from_slice(&identifier.to_be_bytes());
        out[6..8].copy_from_slice(&sequence.to_be_bytes());
        out[HEADER_LEN..total].copy_from_slice(payload);
        let checksum = crate::ipv4::internet_checksum(&out[..total]);
        out[2..4].copy_from_slice(&checksum.to_be_bytes());
        Some(total)
    }

    /// The matching reply for an echo request.
    pub fn echo_reply_for<'p>(header: &IcmpMessage<'p>) -> (u16, u16, &'p [u8]) {
        (header.identifier, header.sequence, header.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_roundtrip() {
        let payload = b"orbita-ping";
        let mut buf = [0u8; HEADER_LEN + 16];
        let n = IcmpMessage::build_echo(IcmpKind::EchoRequest, 0xBEEF, 7, payload, &mut buf)
            .unwrap();
        let msg = IcmpMessage::parse(&buf[..n]).unwrap();
        assert_eq!(msg.kind, IcmpKind::EchoRequest);
        assert_eq!(msg.identifier, 0xBEEF);
        assert_eq!(msg.sequence, 7);
        assert_eq!(msg.payload, &payload[..]);

        let (ident, seq, pl) = IcmpMessage::echo_reply_for(&msg);
        let mut rbuf = [0u8; HEADER_LEN + 16];
        let rn = IcmpMessage::build_echo(IcmpKind::EchoReply, ident, seq, pl, &mut rbuf).unwrap();
        let reply = IcmpMessage::parse(&rbuf[..rn]).unwrap();
        assert_eq!(reply.kind, IcmpKind::EchoReply);
        assert_eq!(reply.payload, &payload[..]);
    }

    #[test]
    fn checksum_rejects_corruption() {
        let mut buf = [0u8; HEADER_LEN + 4];
        IcmpMessage::build_echo(IcmpKind::EchoRequest, 1, 1, b"abcd", &mut buf).unwrap();
        buf[9] ^= 1;
        assert!(IcmpMessage::parse(&buf).is_none());
    }
}
