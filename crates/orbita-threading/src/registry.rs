//! Thread registry used by the runtime.

use alloc::collections::BTreeMap;

use crate::{ThreadId, ThreadSpec, ThreadState};

/// Stored record for a registered thread.
#[derive(Debug, Clone)]
pub struct ThreadRecord {
    pub spec: ThreadSpec,
    pub state: ThreadState,
}

/// Registry of all threads known to the runtime.
#[derive(Debug, Default)]
pub struct ThreadRegistry {
    next_id: u64,
    records: BTreeMap<ThreadId, ThreadRecord>,
}

impl ThreadRegistry {
    /// Create an empty registry.
    pub const fn new() -> Self {
        Self {
            next_id: 0,
            records: BTreeMap::new(),
        }
    }

    /// Register a new thread and return its identifier.
    pub fn register(&mut self, spec: ThreadSpec) -> ThreadId {
        let id = ThreadId::new(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.records.insert(
            id,
            ThreadRecord {
                spec,
                state: ThreadState::Created,
            },
        );
        id
    }

    /// Mark the thread as ready to run.
    pub fn mark_ready(&mut self, id: ThreadId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.state = ThreadState::Ready;
        }
    }

    /// Mark the thread as running.
    pub fn mark_running(&mut self, id: ThreadId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.state = ThreadState::Running;
        }
    }

    /// Mark the thread as blocked.
    pub fn mark_blocked(&mut self, id: ThreadId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.state = ThreadState::Blocked;
        }
    }

    /// Mark the thread as finished.
    pub fn mark_finished(&mut self, id: ThreadId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.state = ThreadState::Finished;
        }
    }

    /// Get a thread record by ID.
    pub fn get(&self, id: ThreadId) -> Option<&ThreadRecord> {
        self.records.get(&id)
    }

    /// Iterate over registered thread IDs.
    pub fn ids(&self) -> impl Iterator<Item = ThreadId> + '_ {
        self.records.keys().copied()
    }

    /// Number of registered threads.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
