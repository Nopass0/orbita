#![no_std]

//! Orbita std — the kernel-facing standard library facade.
//!
//! Everything kernel code needs to feel like ordinary Rust, built strictly
//! on `core` + `alloc` + the in-tree `orbita-*` crates:
//!
//! * owned/heap types: `Box`, `Rc`, `Arc`, `String`, `CString`
//! * collections: `Vec`, `VecDeque`, `BTreeMap`, `BTreeSet`, `BinaryHeap`,
//!   `LinkedList`
//! * smart cells: `Cell`, `RefCell`, `OnceCell` (sync), atomics
//! * `println!` / `print!` macros routed through the platform console
//! * math (via `libm`), diagnostics, memory helpers, sync, tasks
//!
//! `use orbita_std::prelude::*;` pulls in the common surface.

extern crate alloc;

mod console_macros;

#[cfg(test)]
mod tests;

pub mod collections;
pub mod console;
pub mod diagnostics;
pub mod ffi;
pub mod math;
pub mod memory;
pub mod prelude;
pub mod rc;
pub mod sync;
pub mod task;
pub mod time;

// ---------------------------------------------------------------------------
// Direct re-exports of the alloc types the kernel already uses everywhere.
// ---------------------------------------------------------------------------

pub use alloc::{format, string::String, vec, vec::Vec};
pub use alloc::boxed::Box;
pub use alloc::borrow::ToOwned;
pub use alloc::string::ToString;

// Commonly used `core` items so kernel code can `use orbita_std::*` and not
// reach for `core::` paths.
pub use core::clone::Clone;
pub use core::cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd};
pub use core::convert::{From, Into, TryFrom, TryInto};
pub use core::default::Default;
pub use core::fmt::{self, Debug, Display, Write};
pub use core::hash::{Hash, Hasher};
pub use core::iter::{Extend, IntoIterator, Iterator};
pub use core::marker::{Copy, Send, Sized, Sync};
pub use core::mem;
pub use core::ops::{Add, AddAssign, Deref, DerefMut, Div, Drop, Mul, Neg, Rem, Sub};
pub use core::option::Option::{self, None, Some};
pub use core::result::Result::{self, Err, Ok};
pub use core::slice;
pub use core::str;

// Facade modules re-exported at the root for one-stop `orbita_std::*`.
pub use collections::*;
pub use ffi::*;
pub use rc::*;

pub use diagnostics::*;
pub use math::*;
pub use memory::*;

#[allow(unused_imports)]
pub use prelude::*;
pub use sync::*;
pub use task::*;
