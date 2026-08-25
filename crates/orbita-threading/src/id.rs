//! Thread identity primitives.

use core::fmt;

/// Stable identifier for a thread or thread-like unit of execution.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ThreadId(pub u64);

impl ThreadId {
    /// Create a new thread identifier from a raw integer.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tid-{}", self.0)
    }
}
