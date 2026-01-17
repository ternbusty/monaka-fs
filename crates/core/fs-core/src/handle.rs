#[cfg(feature = "thread-safe")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(feature = "std", not(feature = "thread-safe")))]
use std::cell::Cell;

#[cfg(not(feature = "std"))]
use core::cell::Cell;

use crate::types::InodeId;

/// File handle representing an open file descriptor
///
/// For thread-safe builds, `position` uses `AtomicU64` for thread-safe interior mutability.
/// For non-thread-safe builds, `position` uses `Cell<u64>`.
pub struct FileHandle {
    pub inode_id: InodeId,
    /// Current file position
    #[cfg(feature = "thread-safe")]
    pub position: AtomicU64,
    #[cfg(not(feature = "thread-safe"))]
    pub position: Cell<u64>,
    pub flags: u32,
}

impl FileHandle {
    /// Create a new FileHandle
    #[cfg(feature = "thread-safe")]
    pub fn new(inode_id: InodeId, position: u64, flags: u32) -> Self {
        Self {
            inode_id,
            position: AtomicU64::new(position),
            flags,
        }
    }

    #[cfg(not(feature = "thread-safe"))]
    pub fn new(inode_id: InodeId, position: u64, flags: u32) -> Self {
        Self {
            inode_id,
            position: Cell::new(position),
            flags,
        }
    }

    /// Get current position
    #[cfg(feature = "thread-safe")]
    pub fn get_position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    #[cfg(not(feature = "thread-safe"))]
    pub fn get_position(&self) -> u64 {
        self.position.get()
    }

    /// Set position
    #[cfg(feature = "thread-safe")]
    pub fn set_position(&self, pos: u64) {
        self.position.store(pos, Ordering::Relaxed);
    }

    #[cfg(not(feature = "thread-safe"))]
    pub fn set_position(&self, pos: u64) {
        self.position.set(pos);
    }
}
