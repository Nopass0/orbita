#![no_std]

//! Orbita thread identity and lifecycle contracts.
//!
//! This crate does not implement CPU context switching. It defines the data
//! model used by the runtime and scheduler layers when they manage work.

extern crate alloc;

pub mod id;
pub mod registry;
pub mod stack;
pub mod thread;

pub use id::ThreadId;
pub use registry::{ThreadRecord, ThreadRegistry};
pub use stack::StackBounds;
pub use thread::{
    CpuAffinity, ThreadBuilder, ThreadContext, ThreadEntry, ThreadExit, ThreadHandle,
    ThreadPriority, ThreadSpec, ThreadState,
};
