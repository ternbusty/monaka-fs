#[cfg(not(feature = "std"))]
use core::cell::RefCell;

#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, Ordering};

pub trait TimeProvider {
    fn now(&self) -> u64;
}

/// A simple monotonic counter for timestamps.
///
/// For std builds, uses `AtomicU64` for thread safety.
/// For no_std builds, uses `RefCell<u64>`.
pub struct MonotonicCounter {
    #[cfg(feature = "std")]
    counter: AtomicU64,
    #[cfg(not(feature = "std"))]
    counter: RefCell<u64>,
}

impl Default for MonotonicCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicCounter {
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn new() -> Self {
        Self {
            counter: RefCell::new(0),
        }
    }
}

impl TimeProvider for MonotonicCounter {
    #[cfg(feature = "std")]
    fn now(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }

    #[cfg(not(feature = "std"))]
    fn now(&self) -> u64 {
        let mut counter = self.counter.borrow_mut();
        let val = *counter;
        *counter = val + 1;
        val
    }
}
