//! Kernel/OS information.
use alloc::vec;

use alloc::string::String;

use crate::abi::{self, AbiStatus};

/// Kernel/OS summary (version, renderer, memory, CPUs).
pub fn info() -> String {
    let mut length = 0usize;
    let status = (abi::table().os_info)(core::ptr::null_mut(), 0, &mut length);
    if status != AbiStatus::BufferTooSmall as i32 {
        return String::new();
    }
    let mut buffer = vec![0u8; length];
    let mut filled = 0usize;
    let status = (abi::table().os_info)(buffer.as_mut_ptr(), length, &mut filled);
    if status != AbiStatus::Ok as i32 {
        return String::new();
    }
    buffer.truncate(filled);
    String::from_utf8_lossy(&buffer).into_owned()
}
