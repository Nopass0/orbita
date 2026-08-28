//! Monotonic time (ABI v2 syscall transport).

/// Milliseconds since boot (monotonic, wraps at u64).
pub fn now_ms() -> u64 {
    crate::abi::call(crate::abi::nr::TIME_MS, 0, 0, 0, 0)
}
