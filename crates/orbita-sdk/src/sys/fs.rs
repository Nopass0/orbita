//! File and directory access on the live OrbitaFS volume (ABI v2
//! syscall transport).

use alloc::string::String;
use alloc::vec::Vec;
#[allow(unused_imports)]
use alloc::vec;

use crate::abi::{self, AbiStatus, nr};

/// Why a filesystem call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    /// The path does not exist.
    NotFound,
    /// The provided buffer was too small (internal, retried).
    BufferTooSmall,
    /// An I/O error on the volume.
    Io,
    /// The call is unavailable in this kernel build.
    Unsupported,
}

fn map_status(status: i64) -> Result<(), FsError> {
    match status {
        0 => Ok(()),
        s if s == AbiStatus::NotFound as i32 as i64 => Err(FsError::NotFound),
        s if s == AbiStatus::BufferTooSmall as i32 as i64 => Err(FsError::BufferTooSmall),
        s if s == AbiStatus::Unsupported as i32 as i64 => Err(FsError::Unsupported),
        _ => Err(FsError::Io),
    }
}

/// Read a whole file into memory.
pub fn read(path: &str) -> Result<Vec<u8>, FsError> {
    // First call without a buffer: the kernel answers with the length
    // (positive) or a negative status.
    let probe = abi::call(nr::FS_READ, path.as_ptr() as u64, path.len() as u64, 0, 0) as i64;
    if probe < 0 {
        return map_status(probe).map(|_| Vec::new());
    }
    let mut buffer = vec![0u8; probe as usize];
    let filled = abi::call(
        nr::FS_READ,
        path.as_ptr() as u64,
        path.len() as u64,
        buffer.as_mut_ptr() as u64,
        buffer.len() as u64,
    ) as i64;
    if filled < 0 {
        return map_status(filled).map(|_| Vec::new());
    }
    buffer.truncate(filled as usize);
    Ok(buffer)
}

/// Read a whole file as UTF-8 text (lossy).
pub fn read_text(path: &str) -> Result<String, FsError> {
    read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Write (create or replace) a file.
pub fn write(path: &str, data: &[u8]) -> Result<(), FsError> {
    map_status(abi::call(
        nr::FS_WRITE,
        path.as_ptr() as u64,
        path.len() as u64,
        data.as_ptr() as u64,
        data.len() as u64,
    ) as i64)
}

/// List a directory (entries, directories with a trailing `/`).
pub fn list_dir(path: &str) -> Result<Vec<String>, FsError> {
    let probe = abi::call(nr::FS_LIST, path.as_ptr() as u64, path.len() as u64, 0, 0) as i64;
    if probe < 0 {
        return map_status(probe).map(|_| Vec::new());
    }
    let mut buffer = vec![0u8; probe as usize];
    let filled = abi::call(
        nr::FS_LIST,
        path.as_ptr() as u64,
        path.len() as u64,
        buffer.as_mut_ptr() as u64,
        buffer.len() as u64,
    ) as i64;
    if filled < 0 {
        return map_status(filled).map(|_| Vec::new());
    }
    buffer.truncate(filled as usize);
    let text = String::from_utf8_lossy(&buffer).into_owned();
    Ok(text.lines().filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Delete a file or (empty) directory.
pub fn delete(path: &str) -> Result<(), FsError> {
    map_status(abi::call(
        nr::FS_DELETE,
        path.as_ptr() as u64,
        path.len() as u64,
        0,
        0,
    ) as i64)
}
