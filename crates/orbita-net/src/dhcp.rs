//! DHCP client (stage D.1): packet codec + the DISCOVER→OFFER→REQUEST→ACK
//! state machine over UDP ports 67/68.
//!
//! Pure logic — host-tested; the stack feeds inbound datagrams into
//! [`DhcpClient::on_packet`] and transmits what `pending_frame` returns.
//! Renewal timers are the socket layer's business (v1: single-shot lease).

use crate::ipv4::Ipv4Address;
use orbita_std::Vec;
#[allow(unused_imports)]
use orbita_std::vec;

/// Server port.
pub const SERVER_PORT: u16 = 67;
/// Client port.
pub const CLIENT_PORT: u16 = 68;

/// BOOTP/DHCP message operations (option 53).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DhcpOp {
    Discover,
    Offer,
    Request,
    Decline,
    Ack,
    Nak,
    Release,
    Inform,
}

impl DhcpOp {
    fn code(self) -> u8 {
        match self {
            DhcpOp::Discover => 1,
            DhcpOp::Offer => 2,
            DhcpOp::Request => 3,
            DhcpOp::Decline => 4,
            DhcpOp::Ack => 5,
            DhcpOp::Nak => 6,
            DhcpOp::Release => 7,
            DhcpOp::Inform => 8,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => DhcpOp::Discover,
            2 => DhcpOp::Offer,
            3 => DhcpOp::Request,
            4 => DhcpOp::Decline,
            5 => DhcpOp::Ack,
            6 => DhcpOp::Nak,
            7 => DhcpOp::Release,
            8 => DhcpOp::Inform,
            _ => return None,
        })
    }
}

/// A parsed DHCP message (the fields we care about).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DhcpMessage {
    pub op: DhcpOp,
    /// Offered/acknowledged client address (`yiaddr`).
    pub your_address: Ipv4Address,
    /// Server identifier (option 54).
    pub server_id: Option<Ipv4Address>,
    /// Lease time seconds (option 51).
    pub lease_seconds: Option<u32>,
    /// Router (option 3).
    pub router: Option<Ipv4Address>,
    /// Subnet mask (option 1).
    pub subnet_mask: Option<Ipv4Address>,
}

/// Client transaction id (fixed per exchange in v1).
pub type TransactionId = u32;

/// Builds a DHCPDISCOVER or DHCPREQUEST wire message into `out`.
#[allow(clippy::too_many_arguments)]
pub fn build_message(
    op: DhcpOp,
    transaction: TransactionId,
    client_mac: [u8; 6],
    requested_ip: Option<Ipv4Address>,
    server_id: Option<Ipv4Address>,
    out: &mut [u8],
) -> Option<usize> {
    const BASE: usize = 240; // BOOTP header + magic cookie
    if out.len() < BASE + 6 + 3 {
        return None;
    }
    out[..BASE].fill(0);
    out[0] = 1; // BOOTREQUEST
    out[1] = 1; // Ethernet
    out[2] = 6; // hardware address length
    out[4..8].copy_from_slice(&transaction.to_be_bytes());
    out[10] = 0x80; // broadcast flag (no ARP for the reply yet)
    out[28..34].copy_from_slice(&client_mac);
    // Magic cookie.
    out[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
    let mut at = BASE;
    // Option 53: message type.
    out[at..at + 3].copy_from_slice(&[53, 1, op.code()]);
    at += 3;
    // Option 50: requested IP (REQUEST / DISCOVER hint).
    if let Some(ip) = requested_ip {
        out[at] = 50;
        out[at + 1] = 4;
        out[at + 2..at + 6].copy_from_slice(&ip.0);
        at += 6;
    }
    // Option 54: server identifier (REQUEST).
    if let Some(server) = server_id {
        out[at] = 54;
        out[at + 1] = 4;
        out[at + 2..at + 6].copy_from_slice(&server.0);
        at += 6;
    }
    // Option 55: parameter request list (router + mask).
    out[at..at + 5].copy_from_slice(&[55, 3, 1, 3, 6]);
    at += 5;
    // End marker + padding.
    out[at] = 255;
    at += 1;
    Some(at)
}

/// Parses a DHCP message (OFFER/ACK/NAK from the server).
pub fn parse_message(data: &[u8], want_transaction: TransactionId) -> Option<DhcpMessage> {
    if data.len() < 240 || data[0] != 2 {
        return None; // BOOTREPLY only
    }
    if &data[236..240] != &[0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let transaction = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if transaction != want_transaction {
        return None;
    }
    let your_address = Ipv4Address::new([data[16], data[17], data[18], data[19]]);

    let mut op = None;
    let mut server_id = None;
    let mut lease = None;
    let mut router = None;
    let mut mask = None;
    let mut at = 240usize;
    while at + 1 < data.len() {
        let (kind, len) = (data[at], data[at + 1] as usize);
        if kind == 0 {
            at += 1;
            continue;
        }
        if kind == 255 || at + 2 + len > data.len() {
            break;
        }
        let value = &data[at + 2..at + 2 + len];
        match (kind, len) {
            (53, 1) => op = DhcpOp::from_code(value[0]),
            (54, 4) => server_id = Some(Ipv4Address::new([value[0], value[1], value[2], value[3]])),
            (51, 4) => {
                lease = Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
            }
            (3, 4) => router = Some(Ipv4Address::new([value[0], value[1], value[2], value[3]])),
            (1, 4) => mask = Some(Ipv4Address::new([value[0], value[1], value[2], value[3]])),
            _ => {}
        }
        at += 2 + len;
    }
    Some(DhcpMessage {
        op: op?,
        your_address,
        server_id,
        lease_seconds: lease,
        router,
        subnet_mask: mask,
    })
}

/// Client states.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DhcpState {
    /// Nothing sent yet.
    Init,
    /// DISCOVER sent, waiting for an OFFER.
    Selecting,
    /// REQUEST sent, waiting for an ACK.
    Requesting,
    /// Lease acquired.
    Bound,
}

/// What the client wants from the stack right now.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DhcpAction {
    /// Nothing to transmit.
    None,
    /// Transmit this DHCP payload (UDP 68 → 67 broadcast).
    Transmit(Vec<u8>),
    /// Lease acquired — configure the interface.
    Bound {
        address: Ipv4Address,
        router: Option<Ipv4Address>,
        subnet_mask: Option<Ipv4Address>,
        lease_seconds: Option<u32>,
    },
}

/// The DHCP client state machine.
#[derive(Debug)]
pub struct DhcpClient {
    pub state: DhcpState,
    transaction: TransactionId,
    client_mac: [u8; 6],
    offered: Option<Ipv4Address>,
    server: Option<Ipv4Address>,
}

impl DhcpClient {
    pub fn new(client_mac: [u8; 6], transaction: TransactionId) -> Self {
        Self {
            state: DhcpState::Init,
            transaction,
            client_mac,
            offered: None,
            server: None,
        }
    }

    /// Starts (or restarts) the exchange: emits a DHCPDISCOVER payload.
    pub fn start(&mut self) -> DhcpAction {
        let mut payload = vec![0u8; 288];
        let Some(len) = build_message(
            DhcpOp::Discover,
            self.transaction,
            self.client_mac,
            None,
            None,
            &mut payload,
        ) else {
            return DhcpAction::None;
        };
        payload.truncate(len);
        self.state = DhcpState::Selecting;
        DhcpAction::Transmit(payload)
    }

    /// Feeds one inbound server message.
    pub fn on_message(&mut self, message: &DhcpMessage) -> DhcpAction {
        match (self.state, message.op) {
            (DhcpState::Selecting, DhcpOp::Offer) => {
                self.offered = Some(message.your_address);
                self.server = message.server_id;
                let mut payload = vec![0u8; 288];
                let Some(len) = build_message(
                    DhcpOp::Request,
                    self.transaction,
                    self.client_mac,
                    Some(message.your_address),
                    message.server_id,
                    &mut payload,
                ) else {
                    return DhcpAction::None;
                };
                payload.truncate(len);
                self.state = DhcpState::Requesting;
                DhcpAction::Transmit(payload)
            }
            (DhcpState::Requesting, DhcpOp::Ack) => {
                self.state = DhcpState::Bound;
                DhcpAction::Bound {
                    address: message.your_address,
                    router: message.router,
                    subnet_mask: message.subnet_mask,
                    lease_seconds: message.lease_seconds,
                }
            }
            (DhcpState::Requesting, DhcpOp::Nak) | (DhcpState::Selecting, DhcpOp::Nak) => {
                // Server refused: restart the exchange.
                self.start()
            }
            _ => DhcpAction::None,
        }
    }

    /// UDP-filtered inbound hook: the stack calls this for every datagram
    /// from port 67 aimed at port 68.
    pub fn on_packet(&mut self, data: &[u8]) -> DhcpAction {
        match parse_message(data, self.transaction) {
            Some(message) => self.on_message(&message),
            None => DhcpAction::None,
        }
    }

    /// The client's transaction id (diagnostics).
    pub fn transaction_id(&self) -> TransactionId {
        self.transaction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbita_std::vec;

    /// Builds a synthetic server OFFER/ACK wire message.
    fn server_reply(op: DhcpOp, transaction: u32, yiaddr: [u8; 4], server: [u8; 4]) -> Vec<u8> {
        let mut data = vec![0u8; 288];
        data[0] = 2; // BOOTREPLY
        data[4..8].copy_from_slice(&transaction.to_be_bytes());
        data[16..20].copy_from_slice(&yiaddr);
        data[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        let mut at = 240;
        data[at..at + 3].copy_from_slice(&[53, 1, op.code()]);
        at += 3;
        data[at] = 54;
        data[at + 1] = 4;
        data[at + 2..at + 6].copy_from_slice(&server);
        at += 6;
        data[at] = 51;
        data[at + 1] = 4;
        data[at + 2..at + 6].copy_from_slice(&3600u32.to_be_bytes());
        at += 6;
        data[at] = 1; // subnet mask
        data[at + 1] = 4;
        data[at + 2..at + 6].copy_from_slice(&[255, 255, 255, 0]);
        data[at + 6] = 255; // end
        data
    }

    #[test]
    fn discover_builds_and_parses_back() {
        let mut out = [0u8; 256];
        let len = build_message(DhcpOp::Discover, 0x11223344, [1, 2, 3, 4, 5, 6], None, None, &mut out).unwrap();
        let parsed = parse_message(&out[..len], 0x11223344);
        // Discover is a client message: BOOTREQUEST — not parseable as a reply.
        assert!(parsed.is_none());
        assert_eq!(out[0], 1);
        assert_eq!(&out[236..240], &[0x63, 0x82, 0x53, 0x63]);
        assert_eq!(&out[28..34], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn full_exchange_selecting_to_bound() {
        let mut client = DhcpClient::new([1, 2, 3, 4, 5, 6], 7);
        assert_eq!(client.state, DhcpState::Init);
        let DhcpAction::Transmit(discover) = client.start() else { panic!() };
        assert!(discover.len() >= 240);
        assert_eq!(client.state, DhcpState::Selecting);

        // Server OFFER 10.0.2.15 from 10.0.2.2.
        let offer = server_reply(DhcpOp::Offer, 7, [10, 0, 2, 15], [10, 0, 2, 2]);
        let DhcpAction::Transmit(request) = client.on_packet(&offer) else { panic!() };
        assert_eq!(client.state, DhcpState::Requesting);
        // The REQUEST carries the requested-IP option.
        assert!(request.windows(4).any(|w| w == [50, 4, 10, 0]));

        // ACK completes the lease.
        let ack = server_reply(DhcpOp::Ack, 7, [10, 0, 2, 15], [10, 0, 2, 2]);
        let DhcpAction::Bound { address, lease_seconds, subnet_mask, .. } = client.on_packet(&ack)
        else {
            panic!("expected bound")
        };
        assert_eq!(client.state, DhcpState::Bound);
        assert_eq!(address, Ipv4Address::new([10, 0, 2, 15]));
        assert_eq!(lease_seconds, Some(3600));
        assert_eq!(subnet_mask, Some(Ipv4Address::new([255, 255, 255, 0])));
    }

    #[test]
    fn foreign_transaction_is_ignored() {
        let mut client = DhcpClient::new([1, 2, 3, 4, 5, 6], 7);
        client.start();
        let offer = server_reply(DhcpOp::Offer, 999, [10, 0, 2, 15], [10, 0, 2, 2]);
        assert_eq!(client.on_packet(&offer), DhcpAction::None);
        assert_eq!(client.state, DhcpState::Selecting);
    }

    #[test]
    fn nak_restarts_exchange() {
        let mut client = DhcpClient::new([1, 2, 3, 4, 5, 6], 7);
        client.start();
        let offer = server_reply(DhcpOp::Offer, 7, [10, 0, 2, 15], [10, 0, 2, 2]);
        let DhcpAction::Transmit(_) = client.on_packet(&offer) else { panic!() };
        let nak = server_reply(DhcpOp::Nak, 7, [0, 0, 0, 0], [10, 0, 2, 2]);
        let DhcpAction::Transmit(discover) = client.on_packet(&nak) else { panic!() };
        // Back to selecting with a fresh DISCOVER.
        assert_eq!(client.state, DhcpState::Selecting);
        assert!(discover.len() >= 240);
    }

    #[test]
    fn garbage_and_bootrequest_rejected() {
        let mut client = DhcpClient::new([1, 2, 3, 4, 5, 6], 7);
        client.start();
        assert_eq!(client.on_packet(&[0u8; 16]), DhcpAction::None);
        // A BOOTREQUEST aimed at us: rejected by the op check.
        let mut request = vec![0u8; 250];
        request[0] = 1;
        request[4..8].copy_from_slice(&7u32.to_be_bytes());
        request[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        assert_eq!(client.on_packet(&request), DhcpAction::None);
    }
}
