//! Bootstrap semaphore primitive.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Busy-wait semaphore used before sleeping wait queues are available.
pub struct Semaphore {
    permits: AtomicUsize,
}

impl Semaphore {
    /// Create a semaphore with `permits` available permits.
    pub const fn new(permits: usize) -> Self {
        Self {
            permits: AtomicUsize::new(permits),
        }
    }

    /// Try to acquire a permit without blocking.
    pub fn try_acquire(&self) -> bool {
        let mut current = self.permits.load(Ordering::Acquire);
        loop {
            if current == 0 {
                return false;
            }

            match self.permits.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(next) => current = next,
            }
        }
    }

    /// Release a permit back to the semaphore.
    pub fn release(&self) {
        self.permits.fetch_add(1, Ordering::Release);
    }

    /// Return the number of currently available permits.
    pub fn available(&self) -> usize {
        self.permits.load(Ordering::Acquire)
    }
}
