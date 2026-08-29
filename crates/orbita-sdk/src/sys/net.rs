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
    /// Malformed address (expected `ip:port`).
    BadAddress,
    /// Connect failed (no route / stack unavailable).
    ConnectFailed,
}

/// A TCP connection over the kernel socket layer (ABI v2 syscalls).
///
/// v1: snapshot semantics — `read` returns whatever arrived (loopback
/// peers answer synchronously inside the syscall's service rounds).
#[derive(Debug)]
pub struct TcpStream {
    id: u64,
}

fn parse_addr(address: &str) -> Option<(u32, u16)> {
    // `ip:port`
    let (ip, port) = match address.rsplit_once(':') {
        Some((ip, port)) => (ip, port.parse::<u16>().ok()?),
        None => (address, 0),
    };
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut packed = 0u32;
    for part in parts {
        packed = (packed << 8) | part.parse::<u8>().ok()? as u32;
    }
    Some((packed, port))
}

impl TcpStream {
    /// Connects to `ip:port` (dotted quad).
    pub fn connect(address: &str) -> Result<Self, NetError> {
        let Some((ip, port)) = parse_addr(address) else {
            return Err(NetError::BadAddress);
        };
        if port == 0 {
            return Err(NetError::BadAddress);
        }
        let ret = crate::abi::call(nr::SOCKET_CONNECT, ip as u64, port as u64, 0, 0) as i64;
        if ret < 0 {
            return Err(NetError::ConnectFailed);
        }
        Ok(Self { id: ret as u64 })
    }

    /// True once the handshake completed.
    pub fn is_open(&self) -> bool {
        crate::abi::call(nr::SOCKET_STATE, self.id, 0, 0, 0) == 1
    }

    /// Sends `data` (≤512 bytes per call, v1 limit).
    pub fn write(&self, data: &[u8]) -> Result<(), NetError> {
        let ret = crate::abi::call(
            nr::SOCKET_SEND,
            self.id,
            data.as_ptr() as u64,
            data.len() as u64,
            0,
        ) as i64;
        if ret < 0 {
            Err(NetError::Unsupported)
        } else {
            Ok(())
        }
    }

    /// Reads available bytes; `Ok(0)` = nothing yet, `Err` = closed.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, NetError> {
        let ret = crate::abi::call(
            nr::SOCKET_RECV,
            self.id,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            0,
        ) as i64;
        if ret < 0 {
            return Err(NetError::Unsupported);
        }
        Ok(ret as usize)
    }

    /// Closes the connection (FIN).
    pub fn close(&self) {
        let _ = crate::abi::call(nr::SOCKET_CLOSE, self.id, 0, 0, 0);
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
