//! VFS Sync - S3 synchronization for Monaka VFS (native environment).
//!
//! Thin native-side wrapper around [`vfs_sync_core`]. Provides:
//! - The default-HTTP-client variant of [`new_s3_storage`].
//! - The [`HostFs`] newtype implementing [`FsBackend`] for `Arc<Fs>`.
//! - The [`HostSyncManager`] type alias used by `vfs-host`.
//! - An [`init_from_s3`] convenience that builds a fresh `Fs` and populates
//!   it from S3.
//!
//! # Environment Variables
//!
//! - `VFS_S3_BUCKET`: S3 bucket name (required to enable sync)
//! - `VFS_S3_PREFIX`: Key prefix for all objects (default: "vfs/")
//! - `VFS_SYNC_MODE`: "batch" (default) or "realtime"
//! - `AWS_ENDPOINT_URL`: Custom S3 endpoint (LocalStack, MinIO)
//! - `AWS_REGION`: AWS region (default from SDK config)

use std::sync::Arc;

use aws_config::BehaviorVersion;
use fs_core::Fs;
use vfs_sync_core::{FsBackend, S3Error};

pub use vfs_sync_core::{
    populate_from_s3, InboundMode, LoadError, MetadataCache, MetadataMode, S3ObjectInfo, S3Storage,
    SyncConfig, SyncManager, SyncMode, SyncOperation, SyncStats, SyncedFileMetadata,
};

/// Newtype wrapping the thread-safe `Arc<Fs>` so we can implement
/// [`FsBackend`] (orphan rules forbid impls on the bare alias).
#[derive(Clone)]
pub struct HostFs(pub Arc<Fs>);

/// Type alias for the native sync manager.
pub type HostSyncManager = SyncManager<HostFs>;

/// Build an [`S3Storage`] using the AWS SDK's default (hyper-based) HTTP
/// client. Honours `AWS_ENDPOINT_URL` for LocalStack / MinIO compatibility.
pub async fn new_s3_storage(bucket: String, prefix: String) -> S3Storage {
    let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

    if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
        log::debug!("[s3] Using custom endpoint: {}", endpoint);
        config_loader = config_loader.endpoint_url(&endpoint);
    }

    let config = config_loader.load().await;
    S3Storage::from_sdk_config(bucket, prefix, &config)
}

impl FsBackend for HostFs {
    fn open_read(&self, path: &str) -> Result<u32, S3Error> {
        self.0
            .open_path_with_flags(path, fs_core::O_RDONLY)
            .map_err(|e| S3Error::Read {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })
    }

    fn open_write_truncate(&self, path: &str) -> Result<u32, S3Error> {
        self.0
            .open_path_with_flags(path, fs_core::O_RDWR | fs_core::O_CREAT | fs_core::O_TRUNC)
            .map_err(|e| S3Error::Write {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })
    }

    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, S3Error> {
        self.0.read(fd, buf).map_err(|e| S3Error::Read {
            key: format!("fd {}", fd),
            message: format!("Failed to read: {:?}", e),
        })
    }

    fn write(&self, fd: u32, buf: &[u8]) -> Result<usize, S3Error> {
        self.0.write(fd, buf).map_err(|e| S3Error::Write {
            key: format!("fd {}", fd),
            message: format!("Failed to write: {:?}", e),
        })
    }

    fn close(&self, fd: u32) -> Result<(), S3Error> {
        self.0.close(fd).map_err(|e| S3Error::Write {
            key: format!("fd {}", fd),
            message: format!("Failed to close: {:?}", e),
        })
    }

    fn stat_modified(&self, path: &str) -> u64 {
        self.0.stat(path).map(|m| m.modified).unwrap_or(0)
    }

    fn fstat_size(&self, fd: u32) -> Result<u64, S3Error> {
        self.0.fstat(fd).map(|m| m.size).map_err(|e| S3Error::Read {
            key: format!("fd {}", fd),
            message: format!("Failed to stat: {:?}", e),
        })
    }

    fn unlink(&self, path: &str) -> Result<(), S3Error> {
        self.0.unlink(path).map_err(|e| S3Error::Delete {
            key: path.to_string(),
            message: format!("Failed to unlink: {:?}", e),
        })
    }

    fn mkdir_p(&self, path: &str) {
        let _ = self.0.mkdir_p(path);
    }
}

/// Initialise an `Fs` from S3, returning the loaded filesystem (already
/// wrapped in `Arc`) and a fresh `MetadataCache`.
pub async fn init_from_s3(s3: &S3Storage) -> Result<(Arc<Fs>, MetadataCache), LoadError> {
    let fs = Arc::new(Fs::new());
    let cache = populate_from_s3(s3, &HostFs(fs.clone())).await?;
    Ok((fs, cache))
}
