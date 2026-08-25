#![no_std]

//! Orbita scheduling policy.
//!
//! This crate owns ready-queue policy only. It does not perform context
//! switching or interact with hardware timers.

extern crate alloc;

pub mod contract;
pub mod priority;
pub mod round_robin;

pub use contract::{ScheduleDecision, Scheduler};
pub use priority::PriorityClass;
pub use round_robin::RoundRobinScheduler;
