//! S3 synchronization for Halycon VFS (WASI environment)
//!
//! Provides S3 sync capabilities using WASI HTTP for WASM components.
//! Designed for single-threaded WASI environment.

#![allow(warnings)]

mod s3_client;
mod sync_manager;
mod wasi_http;

// Generate WASI bindings
wit_bindgen::generate!({
    world: "vfs-sync-wasi",
    path: "../../../wit",
    generate_all,
});

// Re-export common types from vfs-sync-core
pub use vfs_sync_core::{
    InboundMode, MetadataCache, MetadataMode, S3Error, S3ObjectInfo, SyncConfig, SyncMode,
    SyncOperation, SyncedFileMetadata,
};

// Re-export s3_client and sync_manager types
pub use s3_client::S3Storage;
pub use sync_manager::{init_from_s3, LoadError, SyncManager, SyncStats};
pub use wasi_http::ChunkedWasiHttpClient;
