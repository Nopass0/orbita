//! Network inventory.
//!
//! v1 reports the live interfaces (loopback, e1000 with link state and
//! addresses). Sockets (`TcpStream`/`UdpSocket`) arrive with a later ABI
//! revision — see `docs/roadmap.md`.

use alloc::string::String;
use alloc::vec::Vec;
#[allow(unused_imports)]
use alloc::vec;

use crate::abi::{self, nr};

/// One network interface as reported by the kernel.
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub summary: String,
}

/// List the live network interfaces.
pub fn interfaces() -> Vec<InterfaceInfo> {
    let probe = abi::call(nr::NET_INTERFACES, 0, 0, 0, 0) as i64;
    if probe < 0 {
        return Vec::new();
    }
    let mut buffer: Vec<u8> = vec![0u8; probe as usize];
    let filled =
        abi::call(nr::NET_INTERFACES, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0, 0) as i64;
    if filled < 0 {
        return Vec::new();
    }
    buffer.truncate(filled as usize);
    let text = String::from_utf8_lossy(&buffer).into_owned();
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| InterfaceInfo {
            summary: String::from(line),
        })
        .collect()
}

/// Why a socket operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// Sockets arrive with a later ABI revision.
    Unsupported,
}

/// Placeholder TCP stream API — see module docs.
pub struct TcpStream;

impl TcpStream {
    /// Always [`NetError::Unsupported`] in ABI v1.
    pub fn connect(_address: &str) -> Result<Self, NetError> {
        Err(NetError::Unsupported)
    }
}

/// Placeholder UDP socket API — see module docs.
pub struct UdpSocket;

impl UdpSocket {
    /// Always [`NetError::Unsupported`] in ABI v1.
    pub fn bind(_address: &str) -> Result<Self, NetError> {
        Err(NetError::Unsupported)
    }
}
