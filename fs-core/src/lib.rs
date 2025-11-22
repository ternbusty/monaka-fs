#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod error;
mod time;
mod types;
mod storage;
mod inode;
mod handle;
mod fs;

// Re-export public API
pub use error::FsError;
pub use time::{TimeProvider, MonotonicCounter};
pub use types::{Fd, InodeId, O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, O_TRUNC, O_APPEND, BLOCK_SIZE};
pub use inode::Metadata;
pub use fs::Fs;
