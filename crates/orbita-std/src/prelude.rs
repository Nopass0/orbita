//! The Orbita prelude: everything kernel code commonly needs, in one glob.
//!
//! ```ignore
//! use orbita_std::prelude::*;
//! ```

// Heap types.
pub use crate::{Arc, BinaryHeap, Box, BTreeMap, BTreeSet, CString, LinkedList, Rc, String, ToString, Vec, VecDeque, Weak};

// Macros.
pub use crate::{eprint, eprintln, format, print, println, vec};

// Core essentials.
pub use crate::{
    Clone, Copy, Debug, Default, Display, Drop, Eq, From, Hash, Into, IntoIterator, Iterator,
    None, Ok, Option, Ordering, PartialEq, PartialOrd, Result, Send, Sized, Some, Sync, TryFrom,
    TryInto, Write,
};
