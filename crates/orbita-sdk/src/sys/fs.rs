//! File and directory access on the live OrbitaFS volume.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::abi::{self, AbiStatus, AbiStr};

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

fn str_of(path: &str) -> AbiStr {
    AbiStr {
        ptr: path.as_ptr(),
        len: path.len(),
    }
}

fn map_status(status: i32) -> Result<(), FsError> {
    match status {
        0 => Ok(()),
        s if s == AbiStatus::NotFound as i32 => Err(FsError::NotFound),
        s if s == AbiStatus::BufferTooSmall as i32 => Err(FsError::BufferTooSmall),
        s if s == AbiStatus::Unsupported as i32 => Err(FsError::Unsupported),
        _ => Err(FsError::Io),
    }
}

/// Read a whole file into memory.
pub fn read(path: &str) -> Result<Vec<u8>, FsError> {
    let mut length = 0usize;
    let status = (abi::table().fs_read)(str_of(path), core::ptr::null_mut(), 0, &mut length);
    if status != AbiStatus::BufferTooSmall as i32 {
        return map_status(status).map(|_| Vec::new());
    }
    let mut buffer = vec![0u8; length];
    let mut filled = 0usize;
    let status = (abi::table().fs_read)(str_of(path), buffer.as_mut_ptr(), length, &mut filled);
    map_status(status)?;
    buffer.truncate(filled);
    Ok(buffer)
}

/// Read a whole file as UTF-8 text (lossy).
pub fn read_text(path: &str) -> Result<String, FsError> {
    read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Write (create or replace) a file.
pub fn write(path: &str, data: &[u8]) -> Result<(), FsError> {
    let payload = AbiStr {
        ptr: data.as_ptr(),
        len: data.len(),
    };
    map_status((abi::table().fs_write)(str_of(path), payload))
}

/// List a directory (entries, directories with a trailing `/`).
pub fn list_dir(path: &str) -> Result<Vec<String>, FsError> {
    let mut length = 0usize;
    let status = (abi::table().fs_list)(str_of(path), core::ptr::null_mut(), 0, &mut length);
    if status != AbiStatus::BufferTooSmall as i32 {
        return map_status(status).map(|_| Vec::new());
    }
    let mut buffer = vec![0u8; length];
    let mut filled = 0usize;
    let status = (abi::table().fs_list)(str_of(path), buffer.as_mut_ptr(), length, &mut filled);
    map_status(status)?;
    buffer.truncate(filled);
    let text = String::from_utf8_lossy(&buffer).into_owned();
    Ok(text.lines().filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Delete a file or (empty) directory.
pub fn delete(path: &str) -> Result<(), FsError> {
    map_status((abi::table().fs_delete)(str_of(path)))
}
