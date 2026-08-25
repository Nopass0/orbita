//! Scheduling priority classes.

use core::cmp::Ordering;

/// Ordered priority class used by the scheduler.
///
/// `Realtime` is selected before `High`, and so on.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum PriorityClass {
    Realtime,
    High,
    Normal,
    Low,
    Idle,
}

impl PriorityClass {
    /// Return the numeric rank used for comparisons.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Realtime => 0,
            Self::High => 1,
            Self::Normal => 2,
            Self::Low => 3,
            Self::Idle => 4,
        }
    }
}

impl Ord for PriorityClass {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for PriorityClass {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
