//! Reference-counted pointers re-exported from `alloc`.
//!
//! `Arc` uses the target's atomics (available on x86_64), `Rc` is the
//! single-threaded variant.

pub use alloc::rc::Rc;
pub use alloc::sync::Arc;
pub use alloc::sync::Weak;
