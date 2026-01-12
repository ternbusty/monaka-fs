#[cfg(not(feature = "std"))]
use core::cell::Cell;

#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::InodeId;

/// File handle representing an open file descriptor
///
/// For std builds, `position` uses `AtomicU64` for thread-safe interior mutability.
/// For no_std builds, `position` uses `Cell<u64>`.
pub struct FileHandle {
    pub inode_id: InodeId,
    /// Current file position
    #[cfg(feature = "std")]
    pub position: AtomicU64,
    #[cfg(not(feature = "std"))]
    pub position: Cell<u64>,
    pub flags: u32,
}

impl FileHandle {
    /// Create a new FileHandle
    #[cfg(feature = "std")]
    pub fn new(inode_id: InodeId, position: u64, flags: u32) -> Self {
        Self {
            inode_id,
            position: AtomicU64::new(position),
            flags,
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn new(inode_id: InodeId, position: u64, flags: u32) -> Self {
        Self {
            inode_id,
            position: Cell::new(position),
            flags,
        }
    }

    /// Get current position
    #[cfg(feature = "std")]
    pub fn get_position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    #[cfg(not(feature = "std"))]
    pub fn get_position(&self) -> u64 {
        self.position.get()
    }

    /// Set position
    #[cfg(feature = "std")]
    pub fn set_position(&self, pos: u64) {
        self.position.store(pos, Ordering::Relaxed);
    }

    #[cfg(not(feature = "std"))]
    pub fn set_position(&self, pos: u64) {
        self.position.set(pos);
    }
}
