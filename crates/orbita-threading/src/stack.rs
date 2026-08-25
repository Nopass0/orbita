//! Thread stack bounds and validation helpers.

/// Describes the virtual bounds reserved for a thread stack.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct StackBounds {
    pub base: usize,
    pub size: usize,
}

impl StackBounds {
    /// Create a new stack descriptor.
    pub const fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }

    /// Return the exclusive end address.
    pub const fn end(self) -> usize {
        self.base.saturating_add(self.size)
    }

    /// Check whether an address lies within the stack region.
    pub fn contains(self, addr: usize) -> bool {
        addr >= self.base && addr < self.end()
    }
}
