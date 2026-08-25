//! Cooperative local executor.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use crate::task::{TaskHandle, TaskId, TaskSpec, TaskState};

struct TaskRecord {
    spec: TaskSpec,
    future: Pin<Box<dyn Future<Output = ()> + 'static>>,
    state: TaskState,
}

/// A single-threaded executor intended for early runtime and test execution.
pub struct LocalExecutor {
    tasks: BTreeMap<TaskId, TaskRecord>,
    next_id: u64,
}

impl LocalExecutor {
    /// Create an empty executor.
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_id: 0,
        }
    }

    /// Spawn a future and return a task handle.
    pub fn spawn<F>(&mut self, future: F) -> TaskHandle
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_with_spec(TaskSpec::new("anonymous-task"), future)
    }

    /// Spawn a named task with explicit metadata.
    pub fn spawn_with_spec<F>(&mut self, spec: TaskSpec, future: F) -> TaskHandle
    where
        F: Future<Output = ()> + 'static,
    {
        let id = TaskId::new(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.tasks.insert(
            id,
            TaskRecord {
                spec,
                future: Box::pin(future),
                state: TaskState::Pending,
            },
        );
        TaskHandle { id }
    }

    /// Poll every runnable task once.
    ///
    /// This is intentionally cooperative and deterministic. A future that
    /// returns `Poll::Pending` remains in the task table and will be polled
    /// again on the next pass.
    pub fn run_once(&mut self) -> usize {
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        let mut completed = 0;

        for record in self.tasks.values_mut() {
            if matches!(record.state, TaskState::Completed) {
                continue;
            }

            record.state = TaskState::Running;
            if let Poll::Ready(()) = record.future.as_mut().poll(&mut context) {
                record.state = TaskState::Completed;
                completed += 1;
            } else {
                record.state = TaskState::Pending;
            }
        }

        completed
    }

    /// Drive the executor until all tasks complete or two consecutive
    /// passes complete nothing (a cooperatively-yielding task needs its
    /// second poll; a genuinely blocked task must not spin forever).
    pub fn run_until_idle(&mut self) {
        let mut barren_passes = 0;
        while barren_passes < 2 {
            let completed = self.run_once();
            if completed == 0 {
                barren_passes += 1;
            } else {
                barren_passes = 0;
            }
            if self.completed_tasks() == self.tasks.len() {
                break;
            }
        }
    }

    /// Return the number of tasks that have finished.
    pub fn completed_tasks(&self) -> usize {
        self.tasks
            .values()
            .filter(|task| matches!(task.state, TaskState::Completed))
            .count()
    }

    /// Return the number of tracked tasks.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Check whether the executor has no tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Inspect the state of a task by ID.
    pub fn state(&self, id: TaskId) -> Option<TaskState> {
        self.tasks.get(&id).map(|task| task.state)
    }

    /// Read the static metadata attached to a task.
    pub fn spec(&self, id: TaskId) -> Option<TaskSpec> {
        self.tasks.get(&id).map(|task| task.spec)
    }
}

impl Default for LocalExecutor {
    fn default() -> Self {
        Self::new()
    }
}

fn noop_waker() -> Waker {
    unsafe { Waker::from_raw(noop_raw_waker()) }
}

fn noop_raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &NOOP_WAKER_VTABLE)
}

fn noop_clone(_: *const ()) -> RawWaker {
    noop_raw_waker()
}

fn noop(_: *const ()) {}

static NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
