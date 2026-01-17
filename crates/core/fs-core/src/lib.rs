#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

// Logging support: conditional compilation for zero-cost when disabled
#[cfg(feature = "logging")]
pub use log::{debug, error, info, trace, warn};

#[cfg(not(feature = "logging"))]
#[macro_export]
macro_rules! trace {
    ($($t:tt)*) => {};
}

#[cfg(not(feature = "logging"))]
#[macro_export]
macro_rules! debug {
    ($($t:tt)*) => {};
}

#[cfg(not(feature = "logging"))]
#[macro_export]
macro_rules! info {
    ($($t:tt)*) => {};
}

#[cfg(not(feature = "logging"))]
#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => {};
}

#[cfg(not(feature = "logging"))]
#[macro_export]
macro_rules! error {
    ($($t:tt)*) => {};
}

mod error;
mod handle;
mod inode;
pub mod snapshot;
mod storage;
mod time;
mod types;

// Filesystem implementation: choose based on thread-safe feature
// - thread-safe: Arc<RwLock> + DashMap (for vfs-host, requires Send+Sync)
// - single-threaded: Rc<RefCell> + HashMap (for vfs-adapter, vfs-rpc-server)
#[cfg(feature = "thread-safe")]
mod fs;
#[cfg(not(feature = "thread-safe"))]
#[path = "fs_singlethread.rs"]
mod fs;

// Re-export public API
pub use error::FsError;
pub use fs::Fs;
pub use inode::{FileContent, Inode, Metadata};
pub use storage::BlockStorage;
pub use time::{MonotonicCounter, TimeProvider};
pub use types::{BLOCK_SIZE, Fd, InodeId, O_APPEND, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
