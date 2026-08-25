//! Monotonic time.

/// Milliseconds since boot (monotonic, wraps at u64).
pub fn now_ms() -> u64 {
    (crate::abi::table().time_ms)()
}
