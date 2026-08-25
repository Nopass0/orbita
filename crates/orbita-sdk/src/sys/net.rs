//! Network inventory.
//!
//! v1 reports the live interfaces (loopback, e1000 with link state and
//! addresses). Sockets (`TcpStream`/`UdpSocket`) arrive with a later ABI
//! revision — see `docs/roadmap.md`.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::abi::{self, AbiStatus};

/// One network interface as reported by the kernel.
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub summary: String,
}

/// List the live network interfaces.
pub fn interfaces() -> Vec<InterfaceInfo> {
    let mut length = 0usize;
    let status = (abi::table().net_interfaces)(core::ptr::null_mut(), 0, &mut length);
    if status != AbiStatus::BufferTooSmall as i32 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; length];
    let mut filled = 0usize;
    let status = (abi::table().net_interfaces)(buffer.as_mut_ptr(), length, &mut filled);
    if status != AbiStatus::Ok as i32 {
        return Vec::new();
    }
    buffer.truncate(filled);
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
