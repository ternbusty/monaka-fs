//! In-memory filesystem implementation shared by every Monaka component.
//!
//! A single `Fs` type is exposed in two flavours via the `thread-safe`
//! feature flag:
//!
//! - `thread-safe` (requires `std`): backed by `DashMap` plus
//!   `Arc<RwLock<Inode>>`, used by host-side wasmtime integrations that
//!   need `Send + Sync`.
//! - default: backed by `RefCell<HashMap>` or `RefCell<BTreeMap>` plus
//!   `Rc<RefCell<Inode>>`, used by the WASI components.
//!
//! Both flavours share one `&self` API, so consumers only need to pick
//! the feature flag that matches their target.

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
mod fs;
mod handle;
mod inode;
pub mod snapshot;
mod storage;
mod time;
mod types;

// Re-export public API
pub use error::FsError;
pub use fs::Fs;
pub use inode::{FileContent, Inode, Metadata};
pub use storage::BlockStorage;
pub use time::{MonotonicCounter, TimeProvider};
pub use types::{BLOCK_SIZE, Fd, InodeId, O_APPEND, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
