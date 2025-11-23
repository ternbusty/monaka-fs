#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod error;
mod fs;
mod handle;
mod inode;
mod storage;
mod time;
mod types;

// Re-export public API
pub use error::FsError;
pub use fs::Fs;
pub use inode::Metadata;
pub use time::{MonotonicCounter, TimeProvider};
pub use types::{BLOCK_SIZE, Fd, InodeId, O_APPEND, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
