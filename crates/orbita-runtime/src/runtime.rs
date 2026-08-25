//! Runtime facade combining all execution-layer crates.

use orbita_async::LocalExecutor;
use orbita_scheduler::{PriorityClass, RoundRobinScheduler, ScheduleDecision, Scheduler};
use orbita_sync::{Mutex, OnceCell};
use orbita_threading::{ThreadHandle, ThreadId, ThreadPriority, ThreadRegistry, ThreadSpec};

use crate::policy::RuntimePolicy;

/// Report returned by a runtime tick.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RuntimeTick {
    pub scheduled_thread: Option<ThreadId>,
    pub completed_async_tasks: usize,
}

/// High-level runtime report.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RuntimeReport {
    pub registered_threads: usize,
    pub async_tasks: usize,
}

/// Orbita runtime facade.
pub struct Runtime {
    policy: RuntimePolicy,
    threads: ThreadRegistry,
    scheduler: RoundRobinScheduler,
    executor: LocalExecutor,
    current_thread: OnceCell<ThreadId>,
    tick_count: usize,
    lock: Mutex<()>,
}

impl Runtime {
    /// Create a new runtime with the supplied policy.
    pub fn new(policy: RuntimePolicy) -> Self {
        Self {
            policy,
            threads: ThreadRegistry::new(),
            scheduler: RoundRobinScheduler::new(),
            executor: LocalExecutor::new(),
            current_thread: OnceCell::new(),
            tick_count: 0,
            lock: Mutex::new(()),
        }
    }

    /// Register a thread with the runtime and enqueue it as ready.
    pub fn register_thread(&mut self, spec: ThreadSpec) -> ThreadHandle {
        let id = self.threads.register(spec);
        self.threads.mark_ready(id);
        self.scheduler.enqueue(id, map_priority(spec.priority));
        ThreadHandle { id }
    }

    /// Spawn an async task under the runtime executor.
    pub fn spawn_task<F>(&mut self, future: F) -> orbita_async::TaskHandle
    where
        F: core::future::Future<Output = ()> + 'static,
    {
        self.executor.spawn(future)
    }

    /// Execute one runtime tick.
    pub fn tick(&mut self) -> RuntimeTick {
        let _guard = self.lock.lock();
        self.tick_count = self.tick_count.saturating_add(1);

        let completed = self.executor.run_once();
        let decision = self.scheduler.next();
        let scheduled_thread = apply_schedule(decision, &mut self.threads, &self.current_thread);

        RuntimeTick {
            scheduled_thread,
            completed_async_tasks: completed,
        }
    }

    /// Drive the runtime until the ready queues and async tasks are idle.
    pub fn run_until_idle(&mut self) {
        while self.scheduler.len() > 0 || !self.executor.is_empty() {
            let _ = self.tick();
        }
    }

    /// Build a summary of runtime state.
    pub fn report(&self) -> RuntimeReport {
        RuntimeReport {
            registered_threads: self.threads.len(),
            async_tasks: self.executor.len(),
        }
    }

    /// Read the runtime policy.
    pub fn policy(&self) -> RuntimePolicy {
        self.policy
    }

    /// Return the number of ticks executed so far.
    pub fn tick_count(&self) -> usize {
        self.tick_count
    }

    /// Inspect the current thread if the runtime has selected one.
    pub fn current_thread(&self) -> Option<ThreadId> {
        self.current_thread.get().copied()
    }
}

fn map_priority(priority: ThreadPriority) -> PriorityClass {
    match priority {
        ThreadPriority::Realtime => PriorityClass::Realtime,
        ThreadPriority::High => PriorityClass::High,
        ThreadPriority::Normal => PriorityClass::Normal,
        ThreadPriority::Low => PriorityClass::Low,
        ThreadPriority::Idle => PriorityClass::Idle,
    }
}

fn apply_schedule(
    decision: ScheduleDecision,
    threads: &mut ThreadRegistry,
    current_thread: &OnceCell<ThreadId>,
) -> Option<ThreadId> {
    let next = decision.next?;
    threads.mark_running(next);
    let _ = current_thread.set(next);
    Some(next)
}
