//! TCP endpoint bookkeeping over [`crate::tcp_state`] (stage D.2).
//!
//! [`TcpEndpoint`] pairs a connection's [`TcpControlBlock`] with its
//! address tuple and an in-order receive buffer; the `NetworkStack`
//! (stack.rs) owns the endpoints, demuxes segments into them and turns
//! the state machine's [`TcpAction`]s into frames. Pure data — no I/O,
//! fully host-testable through the stack.

use crate::ipv4::Ipv4Address;
use crate::tcp_state::TcpControlBlock;
use orbita_std::Vec;

/// One TCP connection (or listening slot) owned by the stack.
#[derive(Debug, Clone)]
pub struct TcpEndpoint {
    /// Our address (the interface the connection lives on).
    pub local_ip: Ipv4Address,
    pub local_port: u16,
    /// Peer address; meaningless for a listener.
    pub remote_ip: Ipv4Address,
    pub remote_port: u16,
    /// Connection state machine.
    pub cb: TcpControlBlock,
    /// In-order bytes received from the peer (drained by the app).
    pub rx: Vec<u8>,
    /// Listener that spawned this endpoint (accepted children).
    pub parent: Option<usize>,
}

impl TcpEndpoint {
    /// A fresh listener slot on `(ip, port)`.
    pub fn listener(ip: Ipv4Address, port: u16) -> Self {
        Self {
            local_ip: ip,
            local_port: port,
            remote_ip: Ipv4Address::new([0, 0, 0, 0]),
            remote_port: 0,
            cb: TcpControlBlock::listen(),
            rx: Vec::new(),
            parent: None,
        }
    }
}

/// Address-tuple demux for the stack's endpoint list: exact connection
/// matches first, listeners (any peer) last.
pub fn find_endpoint(
    endpoints: &[TcpEndpoint],
    local_ip: Ipv4Address,
    local_port: u16,
    remote_ip: Ipv4Address,
    remote_port: u16,
) -> Option<usize> {
    endpoints
        .iter()
        .position(|e| {
            e.local_port == local_port
                && e.local_ip == local_ip
                && e.remote_port == remote_port
                && e.remote_ip == remote_ip
        })
        .or_else(|| {
            endpoints
                .iter()
                .position(|e| e.cb.state == crate::tcp_state::TcpState::Listen && e.local_port == local_port)
        })
}
