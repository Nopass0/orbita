use alloc::string::String;
use core::fmt::Write;

pub fn format_bytes(bytes: u64) -> String {
    let mut output = String::new();
    let mib = bytes / (1024 * 1024);
    let kib = (bytes % (1024 * 1024)) / 1024;
    let _ = write!(&mut output, "{mib} MiB {kib} KiB");
    output
}
