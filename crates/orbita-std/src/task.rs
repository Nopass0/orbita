//! Runtime task/thread re-exports.
//!
//! Canonical types live in `orbita-async` (tasks) and `orbita-threading`
//! (threads); `orbita-runtime` composes them into [`Runtime`] and
//! re-exports the full set. This module surfaces the pieces kernel code
//! commonly needs behind the std facade.

pub use orbita_runtime::{
    yield_now, LocalExecutor, Runtime, RuntimePolicy, RuntimeReport, RuntimeTick, TaskHandle,
    TaskId, TaskSpec, TaskState, ThreadPriority, ThreadRegistry,
};
