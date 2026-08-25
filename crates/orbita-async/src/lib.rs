#![no_std]

//! Orbita async execution contracts.
//!
//! The executor here is deliberately cooperative. It provides the smallest
//! useful future-driven runtime surface without assuming timer interrupts or
//! a kernel reactor.

extern crate alloc;

pub mod executor;
pub mod task;
pub mod yield_now;

pub use executor::LocalExecutor;
pub use task::{TaskBudget, TaskHandle, TaskId, TaskSpec, TaskState};
pub use yield_now::{yield_now, YieldNow};
