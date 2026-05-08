//! Common S3 sync logic for Monaka VFS.
//!
//! Holds both the shared types and the S3 / sync-manager logic. Consumers
//! (`vfs-sync-host`, `vfs-sync-adapter`) wire their environment-specific
//! HTTP client and `Fs` handle into the trait abstractions defined here.

mod config;
mod file_metadata;
mod fs_backend;
mod s3_client;
mod sync_manager;
mod types;

pub use config::{InboundMode, MetadataMode, SyncConfig, SyncMode, SyncOperation};
pub use file_metadata::{MetadataCache, SyncedFileMetadata};
pub use fs_backend::{Fd, FsBackend};
pub use s3_client::S3Storage;
pub use sync_manager::{populate_from_s3, LoadError, SyncManager, SyncStats};
pub use types::{S3Error, S3ObjectInfo};
