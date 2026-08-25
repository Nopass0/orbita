//! Async task identity and lifecycle.

use core::fmt;

/// Stable identifier for an async task.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TaskId(pub u64);

impl TaskId {
    /// Create a new task identifier.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task-{}", self.0)
    }
}

/// Lifecycle state tracked for async tasks.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TaskState {
    Pending,
    Running,
    Completed,
}

/// Budget attached to a task.
///
/// This is a policy hint for the runtime and executor. It can be used to cap
/// how much cooperative work a task is allowed to perform per tick.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct TaskBudget {
    pub poll_budget: u32,
}

impl TaskBudget {
    /// Create a task budget with no explicit limit.
    pub const fn unlimited() -> Self {
        Self { poll_budget: 0 }
    }

    /// Create a task budget with a fixed poll limit.
    pub const fn limited(poll_budget: u32) -> Self {
        Self { poll_budget }
    }
}

/// Static task metadata used by the runtime.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct TaskSpec {
    pub name: &'static str,
    pub budget: TaskBudget,
}

impl TaskSpec {
    /// Create a task spec with default unlimited budget.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            budget: TaskBudget::unlimited(),
        }
    }
}

/// Handle returned to the caller when a task is spawned.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TaskHandle {
    pub id: TaskId,
}
