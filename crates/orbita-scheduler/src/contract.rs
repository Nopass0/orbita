//! Scheduler contract types.

use orbita_threading::ThreadId;

/// Result of asking the scheduler for the next runnable thread.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ScheduleDecision {
    pub next: Option<ThreadId>,
}

impl ScheduleDecision {
    /// Create an empty decision.
    pub const fn idle() -> Self {
        Self { next: None }
    }

    /// Create a decision pointing at a runnable thread.
    pub const fn run(next: ThreadId) -> Self {
        Self { next: Some(next) }
    }
}

/// Abstract scheduling interface.
pub trait Scheduler {
    /// Add a runnable thread to the ready queues.
    fn enqueue(&mut self, thread: ThreadId, priority: crate::priority::PriorityClass);

    /// Pop the next runnable thread according to policy.
    fn next(&mut self) -> ScheduleDecision;

    /// Number of queued runnable threads.
    fn len(&self) -> usize;

    /// Whether there are no runnable threads.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
