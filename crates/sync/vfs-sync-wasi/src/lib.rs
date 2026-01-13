//! S3 synchronization for Halycon VFS (WASI environment)
//!
//! Provides S3 sync capabilities using WASI HTTP for WASM components.
//! Designed for single-threaded WASI environment.

#![allow(warnings)]

mod file_metadata;
mod s3_client;
mod sync_manager;
mod wasi_http;

// Generate WASI bindings
wit_bindgen::generate!({
    world: "vfs-sync-wasi",
    path: "wit",
    generate_all,
});

// Re-export public types
pub use file_metadata::{MetadataCache, SyncedFileMetadata};
pub use s3_client::{S3Error, S3ObjectInfo, S3Storage};
pub use sync_manager::{init_from_s3, LoadError, SyncConfig, SyncManager, SyncMode, SyncOperation};
pub use wasi_http::ChunkedWasiHttpClient;
