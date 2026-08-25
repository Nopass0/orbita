//! Time primitives. `Duration` comes from `core`; the helpers below make
//! common kernel conversions ergonomic.

pub use core::time::Duration;

/// Duration in whole milliseconds.
pub const fn duration_millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Duration in whole microseconds.
pub const fn duration_micros(us: u64) -> Duration {
    Duration::from_micros(us)
}

/// Duration in whole nanoseconds.
pub const fn duration_nanos(ns: u64) -> Duration {
    Duration::from_nanos(ns)
}
