//! VFS Sync - S3 synchronization for Halycon VFS
//!
//! This crate provides S3 synchronization capabilities for the Halycon VFS.
//! It is designed for native (non-WASI) environments and uses the standard
//! AWS SDK HTTP client.
//!
//! # Features
//!
//! - **Bidirectional sync**: Upload local changes to S3, download remote changes
//! - **Batch and RealTime modes**: Configure sync timing via `VFS_SYNC_MODE` env var
//! - **Conflict detection**: Uses ETags and timestamps to detect changes
//! - **Thread-safe**: Uses `Arc<Fs>` and `Mutex` for multi-threaded environments
//!
//! # Usage
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use vfs_sync_host::{S3Storage, HostSyncManager, SyncConfig, init_from_s3};
//! use fs_core::Fs;
//!
//! // Initialize from S3
//! let s3 = S3Storage::new("my-bucket".to_string(), "vfs/".to_string()).await;
//! let (fs, metadata_cache) = init_from_s3(&s3).await?;
//! let fs = Arc::new(fs);
//!
//! // Create sync manager
//! let config = SyncConfig::from_env();
//! let sync_manager = Arc::new(HostSyncManager::new(s3, fs.clone(), metadata_cache, config));
//!
//! // In background thread, periodically call:
//! sync_manager.maybe_sync().await;
//!
//! // On file write, enqueue sync:
//! sync_manager.enqueue_upload("/path/to/file".to_string());
//! ```
//!
//! # Environment Variables
//!
//! - `VFS_S3_BUCKET`: S3 bucket name (required to enable sync)
//! - `VFS_S3_PREFIX`: Key prefix for all objects (default: "vfs/")
//! - `VFS_SYNC_MODE`: "batch" (default) or "realtime"
//! - `AWS_ENDPOINT_URL`: Custom S3 endpoint (for LocalStack, MinIO)
//! - `AWS_REGION`: AWS region (default from SDK config)

mod s3_client;
mod sync_manager;

// Re-export common types from vfs-sync-core
pub use vfs_sync_core::{
    InboundMode, MetadataCache, MetadataMode, S3Error, S3ObjectInfo, SyncConfig, SyncMode,
    SyncOperation, SyncedFileMetadata,
};

// Re-export s3_client and sync_manager types
pub use s3_client::S3Storage;
pub use sync_manager::{init_from_s3, HostSyncManager, LoadError, SyncStats};
