//! Spin-based mutual exclusion primitive.

use spin::{Mutex as SpinMutex, MutexGuard as SpinMutexGuard};

/// A small wrapper around `spin::Mutex`.
///
/// The wrapper keeps the public API focused on Orbita's runtime contracts and
/// lets us swap the backend later without changing call sites.
pub struct Mutex<T> {
    inner: SpinMutex<T>,
}

/// Guard returned by [`Mutex::lock`].
pub type MutexGuard<'a, T> = SpinMutexGuard<'a, T>;

impl<T> Mutex<T> {
    /// Create a new mutex protecting `value`.
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinMutex::new(value),
        }
    }

    /// Lock the mutex and return a guard.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.inner.lock()
    }

    /// Try to lock the mutex without blocking.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.inner.try_lock()
    }
}
