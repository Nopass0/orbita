use alloc::{boxed::Box, vec, vec::Vec};

/// Convenience helpers that make the kernel-facing allocator API less verbose.
pub fn boxed_slice(size: usize, fill: u8) -> Box<[u8]> {
    vec![fill; size].into_boxed_slice()
}

pub fn with_capacity<T>(capacity: usize) -> Vec<T> {
    Vec::with_capacity(capacity)
}
