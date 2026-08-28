//! Kernel/OS information (ABI v2 syscall transport).
use alloc::string::String;
use alloc::vec::Vec;
#[allow(unused_imports)]
use alloc::vec;

use crate::abi::{self, nr};

/// Kernel/OS summary (version, renderer, memory, CPUs).
pub fn info() -> String {
    let probe = abi::call(nr::OS_INFO, 0, 0, 0, 0) as i64;
    if probe < 0 {
        return String::new();
    }
    let mut buffer: Vec<u8> = vec![0u8; probe as usize];
    let filled = abi::call(nr::OS_INFO, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0, 0) as i64;
    if filled < 0 {
        return String::new();
    }
    buffer.truncate(filled as usize);
    String::from_utf8_lossy(&buffer).into_owned()
}
