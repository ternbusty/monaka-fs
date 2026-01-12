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
}

/// No-op implementation when sync is disabled
pub struct NoOpSyncHooks;

impl SyncHooks for NoOpSyncHooks {
    fn on_write(&self, _path: &str) {}
    fn on_delete(&self, _path: &str) {}
    fn on_truncate(&self, _path: &str) {}
}

/// S3 sync hooks implementation
#[cfg(feature = "s3-sync")]
pub struct S3SyncHooks {
    sync_manager: Arc<HostSyncManager>,
}

#[cfg(feature = "s3-sync")]
impl S3SyncHooks {
    /// Create new S3 sync hooks with the given sync manager
    pub fn new(sync_manager: Arc<HostSyncManager>) -> Self {
        Self { sync_manager }
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
}
