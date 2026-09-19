//! Common S3 sync logic for Monaka VFS.
//!
//! Holds both the shared types and the S3 / sync-manager logic. Consumers
//! (`vfs-sync-host`, `vfs-sync-adapter`) wire their environment-specific
//! HTTP client and `Fs` handle into the trait abstractions defined here.
//!
//! # Multi-instance safety
//!
//! Several instances may sync the same bucket and prefix. Data safety rests
//! on two mechanisms, both built on S3's atomically evaluated conditional
//! requests.
//!
//! * Every `PutObject` / `CompleteMultipartUpload` / `DeleteObject` carries
//!   `If-Match` with the ETag this instance last observed (or
//!   `If-None-Match: *` for a new object). A write based on stale state is
//!   rejected by S3 instead of silently overwriting.
//! * With `VFS_S3_FILE_LOCK=enabled` (the default), opening a file for
//!   write acquires a per-file lease stored at `locks/<path>`, refreshes the
//!   local copy from S3, and pins the data ETag. The lease is held until the
//!   last local write descriptor closes, at which point the content is
//!   pushed with `If-Match` and the lease released. Other instances opening
//!   the same file for write wait up to `VFS_S3_FILE_LOCK_TIMEOUT_MS` and
//!   then receive [`SyncError::Busy`].
//!
//! Inbound change detection compares ETags only. Wall-clock timestamps are
//! never used to decide which side is newer.
//!
//! # Fencing gap
//!
//! A holder whose lease expired (it stopped renewing) may still complete a
//! `PutObject` before the new holder writes. The new holder's own write then
//! fails with 412 and is reported as [`SyncError::Conflict`] after the
//! local copy has been refreshed. No data is lost; the window is bounded by
//! `VFS_S3_FILE_LOCK_LEASE_SECS` and stays closed while the holder is alive
//! because leases are renewed from `maybe_sync`.

mod config;
mod file_metadata;
mod fs_backend;
mod lease;
mod object_store;
mod s3_client;
mod sync_manager;
pub mod testing;
mod types;

pub use config::{InboundMode, MetadataMode, SyncConfig, SyncMode, SyncOperation};
pub use file_metadata::{MetadataCache, SyncedFileMetadata};
pub use fs_backend::{Fd, FsBackend};
pub use lease::LockRecord;
pub use object_store::{
    dir_marker_key, file_key, lock_key, path_from_file_key, FileStore, ObjectStore, FILES_PREFIX,
    LOCKS_PREFIX,
};
pub use s3_client::S3Storage;
pub use sync_manager::{populate_from_s3, LoadError, SyncManager, SyncStats};
pub use types::{ObjectMeta, Precondition, S3Error, S3ObjectInfo, SyncError};
