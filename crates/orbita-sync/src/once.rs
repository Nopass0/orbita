//! One-time initialization cell.

use core::cell::UnsafeCell;
use core::ops::Deref;
use core::sync::atomic::{AtomicBool, Ordering};

/// A simple single-assignment cell for boot-time and runtime initialization.
///
/// This is intentionally small and explicit. The cell is designed for cases
/// where a value is written once during startup and then read many times.
pub struct OnceCell<T> {
    ready: AtomicBool,
    value: UnsafeCell<Option<T>>,
}

unsafe impl<T: Send + Sync> Sync for OnceCell<T> {}

impl<T> OnceCell<T> {
    /// Create an empty cell.
    pub const fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            value: UnsafeCell::new(None),
        }
    }

    /// Store a value if the cell is still empty.
    pub fn set(&self, value: T) -> Result<(), T> {
        if self
            .ready
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(value);
        }

        unsafe {
            *self.value.get() = Some(value);
        }
        Ok(())
    }

    /// Get a shared reference if the cell has already been initialized.
    pub fn get(&self) -> Option<&T> {
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }

        unsafe { (*self.value.get()).as_ref() }
    }

    /// Initialize the cell on demand and return a shared reference.
    pub fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        if let Some(value) = self.get() {
            return value;
        }

        let value = init();
        let _ = self.set(value);
        self.get().expect("OnceCell must be initialized")
    }
}

impl<T> Deref for OnceCell<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get().expect("OnceCell is not initialized")
    }
}
