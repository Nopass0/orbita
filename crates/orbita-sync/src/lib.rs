#![no_std]

//! Orbita synchronization primitives.
//!
//! This crate defines the small set of synchronization types used by the
//! runtime and scheduler layers. The goal is to keep the API explicit and
//! keep the implementation hidden behind predictable contracts.

pub mod mutex;
pub mod event;
pub mod once;
pub mod rwlock;
pub mod semaphore;

pub use event::Event;
pub use mutex::{Mutex, MutexGuard};
pub use once::OnceCell;
pub use rwlock::{RwLock, RwLockReadGuard, RwLockWriteGuard};
pub use semaphore::Semaphore;
