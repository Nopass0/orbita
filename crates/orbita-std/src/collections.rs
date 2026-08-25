//! Heap-backed collections re-exported from `alloc`.
//!
//! The kernel deliberately keeps `BTreeMap`/`BTreeSet` as its primary
//! map/set types: they need no random state and therefore no entropy
//! source, which a young kernel cannot guarantee yet.

pub use alloc::collections::{BinaryHeap, BTreeMap, BTreeSet, LinkedList, VecDeque};
