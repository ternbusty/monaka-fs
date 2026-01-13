//! Sync hooks for triggering S3 sync on filesystem mutations
//!
//! This module provides the `SyncHooks` trait and implementations for
//! notifying the sync manager when files are modified.

use std::sync::Arc;

#[cfg(feature = "s3-sync")]
use vfs_sync::HostSyncManager;

/// Trait for receiving filesystem mutation notifications
///
/// Implementations can use these hooks to trigger S3 synchronization
/// or other side effects when files are modified.
pub trait SyncHooks: Send + Sync {
    /// Called after a successful write operation
    fn on_write(&self, path: &str);
    /// Called after a file is deleted
    fn on_delete(&self, path: &str);
    /// Called after a file is truncated
    fn on_truncate(&self, path: &str);
    /// Called before a read operation to refresh from S3
    fn on_read(&self, path: &str);
    /// Called before opening a file to check S3 metadata (like s3fs HEAD request)
    fn on_open(&self, path: &str);
}

/// No-op implementation when sync is disabled
pub struct NoOpSyncHooks;

impl SyncHooks for NoOpSyncHooks {
    fn on_write(&self, _path: &str) {}
    fn on_delete(&self, _path: &str) {}
    fn on_truncate(&self, _path: &str) {}
    fn on_read(&self, _path: &str) {}
    fn on_open(&self, _path: &str) {}
}

/// S3 sync hooks implementation
#[cfg(feature = "s3-sync")]
pub struct S3SyncHooks {
    sync_manager: Arc<HostSyncManager>,
    /// Whether to refresh files from S3 on read
    read_from_s3: bool,
    /// Whether to check S3 metadata on open (like s3fs HEAD request)
    metadata_sync: bool,
}

#[cfg(feature = "s3-sync")]
impl S3SyncHooks {
    /// Create new S3 sync hooks with the given sync manager
    pub fn new(sync_manager: Arc<HostSyncManager>) -> Self {
        Self {
            sync_manager,
            read_from_s3: false,
            metadata_sync: false,
        }
    }

    /// Create new S3 sync hooks with read-from-S3 option
    pub fn new_with_read_mode(sync_manager: Arc<HostSyncManager>, read_from_s3: bool) -> Self {
        Self {
            sync_manager,
            read_from_s3,
            metadata_sync: false,
        }
    }

    /// Create new S3 sync hooks with all options
    pub fn new_with_options(
        sync_manager: Arc<HostSyncManager>,
        read_from_s3: bool,
        metadata_sync: bool,
    ) -> Self {
        Self {
            sync_manager,
            read_from_s3,
            metadata_sync,
        }
    }
}

#[cfg(feature = "s3-sync")]
impl SyncHooks for S3SyncHooks {
    fn on_write(&self, path: &str) {
        if self.sync_manager.is_realtime() {
            // RealTime mode: sync immediately and wait for S3 completion
            // Use a new thread with its own tokio runtime to avoid blocking the main runtime
            let sync = self.sync_manager.clone();
            let path = path.to_string();
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    if let Err(e) = sync.sync_file_now(&path).await {
                        log::error!("[sync] RealTime sync failed for {}: {}", path, e);
                    }
                });
            });
            // Wait for S3 sync to complete before returning
            if let Err(e) = handle.join() {
                log::error!("[sync] RealTime sync thread panicked: {:?}", e);
            }
        } else {
            // Batch mode: enqueue for later
            self.sync_manager.enqueue_upload(path.to_string());
        }
    }

    fn on_delete(&self, path: &str) {
        self.sync_manager.enqueue_delete(path.to_string());
    }

    fn on_truncate(&self, path: &str) {
        if self.sync_manager.is_realtime() {
            // RealTime mode: sync immediately and wait for S3 completion
            let sync = self.sync_manager.clone();
            let path = path.to_string();
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    if let Err(e) = sync.sync_file_now(&path).await {
                        log::error!("[sync] RealTime sync failed for {}: {}", path, e);
                    }
                });
            });
            // Wait for S3 sync to complete before returning
            if let Err(e) = handle.join() {
                log::error!("[sync] RealTime sync thread panicked: {:?}", e);
            }
        } else {
            // Batch mode: enqueue for later
            self.sync_manager.enqueue_upload(path.to_string());
        }
    }

    fn on_read(&self, path: &str) {
        if self.read_from_s3 {
            // Refresh from S3 before read
            let sync = self.sync_manager.clone();
            let path = path.to_string();
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    if let Err(e) = sync.refresh_file_from_s3(&path).await {
                        log::error!("[sync] S3 refresh failed for {}: {}", path, e);
                    }
                });
            });
            // Wait for S3 refresh to complete before returning
            if let Err(e) = handle.join() {
                log::error!("[sync] S3 refresh thread panicked: {:?}", e);
            }
        }
    }

    fn on_open(&self, path: &str) {
        if self.metadata_sync {
            // Check S3 metadata and refresh if changed (like s3fs HEAD request)
            let sync = self.sync_manager.clone();
            let path = path.to_string();
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async {
                    match sync.check_and_refresh_from_s3(&path).await {
                        Ok(refreshed) => {
                            if refreshed {
                                log::debug!("[sync] File refreshed on open: {}", path);
                            }
                        }
                        Err(e) => {
                            log::error!("[sync] S3 metadata check failed for {}: {}", path, e);
                        }
                    }
                });
            });
            // Wait for metadata check to complete before returning
            if let Err(e) = handle.join() {
                log::error!("[sync] S3 metadata check thread panicked: {:?}", e);
            }
        }
    }
}
