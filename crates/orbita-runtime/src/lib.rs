#![no_std]

//! Orbita runtime facade.
//!
//! This crate composes the execution layer into one contract:
//!
//! - [`Runtime`] — threads ([`orbita_threading`]), scheduling policy
//!   ([`orbita_scheduler`]), and cooperative async execution
//!   ([`orbita_async`]) driven together by [`Runtime::tick`].
//! - [`LocalExecutor`] — the real cooperative `Future` executor, re-exported
//!   from `orbita-async` so kernel code needs a single dependency.
//! - [`RuntimePolicy`] — execution budgets for early boot.
//!
//! The former fake "tick-budget executor" compatibility layer was removed:
//! `TaskId`/`TaskSpec`/`TaskState` come from `orbita-async`, thread types
//! (`CpuAffinity`, `ThreadPriority`, `ThreadBuilder`, ...) come from
//! `orbita-threading`, and this crate only adds the composition facade.

extern crate alloc;

pub mod policy;
pub mod runtime;

pub use orbita_async::{yield_now, LocalExecutor, TaskBudget, TaskHandle, TaskId, TaskSpec, TaskState};
pub use orbita_threading::{
    CpuAffinity, ThreadBuilder, ThreadContext, ThreadEntry, ThreadExit, ThreadHandle,
    ThreadPriority, ThreadRecord, ThreadRegistry, ThreadSpec, ThreadState,
};

pub use policy::{RuntimeBudget, RuntimePolicy};
pub use runtime::{Runtime, RuntimeReport, RuntimeTick};
