//! Reader-writer lock wrapper.

use spin::{RwLock as SpinRwLock, RwLockReadGuard as SpinRwLockReadGuard, RwLockWriteGuard as SpinRwLockWriteGuard};

/// A lightweight readers-writer lock for shared runtime state.
pub struct RwLock<T> {
    inner: SpinRwLock<T>,
}

/// Guard returned by [`RwLock::read`].
pub type RwLockReadGuard<'a, T> = SpinRwLockReadGuard<'a, T>;

/// Guard returned by [`RwLock::write`].
pub type RwLockWriteGuard<'a, T> = SpinRwLockWriteGuard<'a, T>;

impl<T> RwLock<T> {
    /// Create a new lock around `value`.
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinRwLock::new(value),
        }
    }

    /// Borrow the protected data for reading.
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read()
    }

    /// Borrow the protected data for writing.
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.inner.write()
    }
}
