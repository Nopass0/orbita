//! Thread metadata and lifecycle contracts.

use crate::{StackBounds, ThreadId};

/// Scheduling priority used by the threading and scheduler layers.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ThreadPriority {
    Realtime,
    High,
    Normal,
    Low,
    Idle,
}

/// CPU selection hint for a thread.
///
/// The runtime can treat this as a soft policy until true affinity management
/// is wired in.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CpuAffinity {
    Any,
    BootstrapCpu,
    Exact(u32),
}

/// State tracked for a thread in the registry.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ThreadState {
    Created,
    Ready,
    Running,
    Blocked,
    Sleeping,
    Finished,
    Panicked,
}

/// Result returned from a thread entry function.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ThreadExit {
    Yield,
    Blocked,
    Finished,
}

/// Execution context passed to thread entry points.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ThreadContext {
    pub id: ThreadId,
}

/// Entry function for a thread.
pub type ThreadEntry = fn(ThreadContext) -> ThreadExit;

/// A handle that can be kept by callers once a thread has been registered.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ThreadHandle {
    pub id: ThreadId,
}

/// Complete thread specification used when registering a thread.
#[derive(Debug, Copy, Clone)]
pub struct ThreadSpec {
    pub name: &'static str,
    pub priority: ThreadPriority,
    pub affinity: CpuAffinity,
    pub stack: StackBounds,
    pub entry: ThreadEntry,
}

/// Builder for thread specifications.
pub struct ThreadBuilder {
    name: &'static str,
    priority: ThreadPriority,
    affinity: CpuAffinity,
    stack: StackBounds,
}

impl ThreadBuilder {
    /// Start with the default thread policy.
    pub const fn new(name: &'static str, stack: StackBounds) -> Self {
        Self {
            name,
            priority: ThreadPriority::Normal,
            affinity: CpuAffinity::Any,
            stack,
        }
    }

    /// Override the thread priority.
    pub const fn priority(mut self, priority: ThreadPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Override the CPU affinity hint.
    pub const fn affinity(mut self, affinity: CpuAffinity) -> Self {
        self.affinity = affinity;
        self
    }

    /// Finalize the spec with a concrete entry function.
    pub const fn build(self, entry: ThreadEntry) -> ThreadSpec {
        ThreadSpec {
            name: self.name,
            priority: self.priority,
            affinity: self.affinity,
            stack: self.stack,
            entry,
        }
    }
}
