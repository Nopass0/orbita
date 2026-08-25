//! Simple event flag for boot and runtime handoff.

use core::sync::atomic::{AtomicBool, Ordering};

/// A tiny event primitive for signaling between subsystems.
///
/// This is useful in bootstrap code where a full wait queue does not yet
/// exist but state still needs to be represented explicitly.
pub struct Event {
    signaled: AtomicBool,
}

impl Event {
    /// Create a non-signaled event.
    pub const fn new() -> Self {
        Self {
            signaled: AtomicBool::new(false),
        }
    }

    /// Mark the event as signaled.
    pub fn signal(&self) {
        self.signaled.store(true, Ordering::Release);
    }

    /// Clear the event state.
    pub fn clear(&self) {
        self.signaled.store(false, Ordering::Release);
    }

    /// Check whether the event has been signaled.
    pub fn is_signaled(&self) -> bool {
        self.signaled.load(Ordering::Acquire)
    }
}
