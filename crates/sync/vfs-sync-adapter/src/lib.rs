//! S3 synchronization for Monaka VFS (WASI environment).
//!
//! Thin WASI-side wrapper around [`vfs_sync_core`]. Provides:
//! - [`new_s3_storage`] using the [`ChunkedWasiHttpClient`].
//! - The [`AdapterFs`] newtype implementing [`FsBackend`] for
//!   `Rc<RefCell<Fs<T>>>`.
//! - Type aliases for the WASI sync manager.

#![allow(warnings)]

mod wasi_http;

use std::cell::RefCell;
use std::rc::Rc;

use aws_config::BehaviorVersion;
use aws_smithy_async::rt::sleep::TokioSleep;
use fs_core::{Fs, TimeProvider};
use vfs_sync_core::{FsBackend, S3Error};

pub use wasi_http::ChunkedWasiHttpClient;

pub use vfs_sync_core::{
    populate_from_s3, InboundMode, LoadError, MetadataCache, MetadataMode, S3ObjectInfo, S3Storage,
    SyncConfig, SyncManager as CoreSyncManager, SyncMode, SyncOperation, SyncStats,
    SyncedFileMetadata,
};

// Generate WASI bindings.
wit_bindgen::generate!({
    world: "vfs-sync-adapter",
    path: "../../../wit",
    generate_all,
});

/// Newtype wrapping the single-threaded `Rc<RefCell<Fs<T>>>` so we can
/// implement [`FsBackend`] (orphan rules forbid impls on the bare alias).
pub struct AdapterFs<T: TimeProvider>(pub Rc<RefCell<Fs<T>>>);

impl<T: TimeProvider> Clone for AdapterFs<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Type alias for the WASI sync manager.
pub type SyncManager<T> = CoreSyncManager<AdapterFs<T>>;

/// Build an [`S3Storage`] using the WASI HTTP client. Honours
/// `AWS_ENDPOINT_URL` for LocalStack / MinIO compatibility.
pub async fn new_s3_storage(bucket: String, prefix: String) -> S3Storage {
    let http_client = ChunkedWasiHttpClient::new();
    let sleep = TokioSleep::new();

    let mut config_loader = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
        .sleep_impl(sleep);

    if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
        log::debug!("[s3] Using custom endpoint: {}", endpoint);
        config_loader = config_loader.endpoint_url(&endpoint);
    }

    let config = config_loader.load().await;
    S3Storage::from_sdk_config(bucket, prefix, &config)
}

impl<T: TimeProvider> FsBackend for AdapterFs<T> {
    fn open_read(&self, path: &str) -> Result<u32, S3Error> {
        self.0
            .borrow_mut()
            .open_path_with_flags(path, fs_core::O_RDONLY)
            .map_err(|e| S3Error::Read {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })
    }

    fn open_write_truncate(&self, path: &str) -> Result<u32, S3Error> {
        self.0
            .borrow_mut()
            .open_path_with_flags(path, fs_core::O_RDWR | fs_core::O_CREAT | fs_core::O_TRUNC)
            .map_err(|e| S3Error::Write {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })
    }

    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, S3Error> {
        self.0
            .borrow_mut()
            .read(fd, buf)
            .map_err(|e| S3Error::Read {
                key: format!("fd {}", fd),
                message: format!("Failed to read: {:?}", e),
            })
    }

    fn write(&self, fd: u32, buf: &[u8]) -> Result<usize, S3Error> {
        self.0
            .borrow_mut()
            .write(fd, buf)
            .map_err(|e| S3Error::Write {
                key: format!("fd {}", fd),
                message: format!("Failed to write: {:?}", e),
            })
    }

    fn close(&self, fd: u32) -> Result<(), S3Error> {
        self.0.borrow_mut().close(fd).map_err(|e| S3Error::Write {
            key: format!("fd {}", fd),
            message: format!("Failed to close: {:?}", e),
        })
    }

    fn stat_modified(&self, path: &str) -> u64 {
        self.0.borrow().stat(path).map(|m| m.modified).unwrap_or(0)
    }

    fn fstat_size(&self, fd: u32) -> Result<u64, S3Error> {
        self.0
            .borrow()
            .fstat(fd)
            .map(|m| m.size)
            .map_err(|e| S3Error::Read {
                key: format!("fd {}", fd),
                message: format!("Failed to stat: {:?}", e),
            })
    }

    fn unlink(&self, path: &str) -> Result<(), S3Error> {
        self.0
            .borrow_mut()
            .unlink(path)
            .map_err(|e| S3Error::Delete {
                key: path.to_string(),
                message: format!("Failed to unlink: {:?}", e),
            })
    }

    fn mkdir_p(&self, path: &str) {
        let _ = self.0.borrow_mut().mkdir_p(path);
    }
}

/// Initialise a fresh `Fs<T>` from S3, returning the populated handle
/// (already wrapped in `Rc<RefCell<…>>`) and metadata cache.
pub async fn init_from_s3<T: TimeProvider + Default>(
    s3: &S3Storage,
) -> Result<(Rc<RefCell<Fs<T>>>, MetadataCache), LoadError> {
    let fs = Rc::new(RefCell::new(Fs::with_time_provider(T::default())));
    let cache = populate_from_s3(s3, &AdapterFs(fs.clone())).await?;
    Ok((fs, cache))
}
