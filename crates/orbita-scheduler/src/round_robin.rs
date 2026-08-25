//! Priority-aware round-robin scheduler.

use alloc::collections::{BTreeMap, VecDeque};

use crate::contract::{ScheduleDecision, Scheduler};
use crate::priority::PriorityClass;
use orbita_threading::ThreadId;

/// Simple scheduler that round-robins within each priority class.
///
/// The scheduler always prefers higher priority queues and rotates items
/// within the selected queue to keep the policy fair.
pub struct RoundRobinScheduler {
    queues: BTreeMap<PriorityClass, VecDeque<ThreadId>>,
    len: usize,
}

impl RoundRobinScheduler {
    /// Create an empty scheduler.
    pub fn new() -> Self {
        let mut queues = BTreeMap::new();
        queues.insert(PriorityClass::Realtime, VecDeque::new());
        queues.insert(PriorityClass::High, VecDeque::new());
        queues.insert(PriorityClass::Normal, VecDeque::new());
        queues.insert(PriorityClass::Low, VecDeque::new());
        queues.insert(PriorityClass::Idle, VecDeque::new());

        Self { queues, len: 0 }
    }

    fn queue_mut(&mut self, priority: PriorityClass) -> &mut VecDeque<ThreadId> {
        self.queues
            .get_mut(&priority)
            .expect("priority queue must exist")
    }
}

impl Default for RoundRobinScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler for RoundRobinScheduler {
    fn enqueue(&mut self, thread: ThreadId, priority: PriorityClass) {
        self.queue_mut(priority).push_back(thread);
        self.len += 1;
    }

    fn next(&mut self) -> ScheduleDecision {
        for priority in [
            PriorityClass::Realtime,
            PriorityClass::High,
            PriorityClass::Normal,
            PriorityClass::Low,
            PriorityClass::Idle,
        ] {
            if let Some(thread) = self.queue_mut(priority).pop_front() {
                self.len -= 1;
                return ScheduleDecision::run(thread);
            }
        }

        ScheduleDecision::idle()
    }

    fn len(&self) -> usize {
        self.len
    }
}
