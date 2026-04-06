//! Common types for VFS S3 synchronization
//!
//! This crate provides shared types used by both native (vfs-sync-host)
//! and WASI (vfs-sync-adapter) sync implementations.

mod config;
mod file_metadata;
mod types;

pub use config::{InboundMode, MetadataMode, SyncConfig, SyncMode, SyncOperation};
pub use file_metadata::{MetadataCache, SyncedFileMetadata};
pub use types::{S3Error, S3ObjectInfo};
