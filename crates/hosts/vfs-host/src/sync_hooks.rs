//! Sync hooks for triggering S3 sync on filesystem mutations
//!
//! This module provides the `SyncHooks` trait and implementations for
//! notifying the sync manager when files are opened, modified, and closed.
//! With `VFS_S3_FILE_LOCK=enabled` the open and close hooks acquire and
//! release the per-file S3 lease; errors from those two hooks (and from
//! `on_fsync`) are the only sync errors an application can observe.

use std::sync::Arc;

#[cfg(feature = "s3-sync")]
use vfs_sync_host::{HostSyncManager, SyncError};

/// How a descriptor was opened, as far as the sync layer cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    Read,
    Write,
}

impl OpenKind {
    /// Derive from fs-core open flags (write access is the low two bits).
    pub fn from_fs_flags(flags: u32) -> Self {
        if flags & 0x3 != fs_core::O_RDONLY {
            OpenKind::Write
        } else {
            OpenKind::Read
        }
    }

    pub fn is_write(self) -> bool {
        matches!(self, OpenKind::Write)
    }
}

/// Errors a hook can report back to the WASI layer.
#[derive(Debug, Clone)]
pub enum SyncHookError {
    /// Another instance holds the file's S3 lease (`error-code::busy`).
    Busy,
    /// The object changed in S3 under the lease
    /// (`error-code::not-recoverable`); the local copy was refreshed.
    Conflict,
    /// Any other sync failure (`error-code::io`).
    Io(String),
}

#[cfg(feature = "s3-sync")]
impl From<SyncError> for SyncHookError {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::Busy { .. } => SyncHookError::Busy,
            SyncError::Conflict { .. } => SyncHookError::Conflict,
            other => SyncHookError::Io(other.to_string()),
        }
    }
}

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
    /// Called before a file is opened. For write opens this is where the
    /// S3 lease is taken; an error aborts the open.
    fn on_open(&self, path: &str, kind: OpenKind) -> Result<(), SyncHookError>;
    /// Called when a descriptor is dropped. For write descriptors this
    /// pushes pending content and releases the lease.
    fn on_close(&self, _path: &str, _kind: OpenKind) -> Result<(), SyncHookError> {
        Ok(())
    }
    /// Called on `sync` / `sync-data`. Pushes pending content without
    /// releasing the lease.
    fn on_fsync(&self, _path: &str, _kind: OpenKind) -> Result<(), SyncHookError> {
        Ok(())
    }
    /// Called after a directory is created.
    fn on_mkdir(&self, _path: &str) -> Result<(), SyncHookError> {
        Ok(())
    }
    /// Called after a directory is removed.
    fn on_rmdir(&self, _path: &str) -> Result<(), SyncHookError> {
        Ok(())
    }
}

/// No-op implementation when sync is disabled
pub struct NoOpSyncHooks;

impl SyncHooks for NoOpSyncHooks {
    fn on_write(&self, _path: &str) {}
    fn on_delete(&self, _path: &str) {}
    fn on_truncate(&self, _path: &str) {}
    fn on_read(&self, _path: &str) {}
    fn on_open(&self, _path: &str, _kind: OpenKind) -> Result<(), SyncHookError> {
        Ok(())
    }
}

/// Runs `task` on a dedicated thread with its own tokio runtime and blocks
/// until it completes, returning the task's output. A separate thread is
/// used so the caller may itself be running inside a tokio runtime.
/// `description` labels error logs when the thread fails.
#[cfg(feature = "s3-sync")]
fn run_sync_task<F, Fut, R>(description: &str, task: F) -> Result<R, SyncHookError>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R>,
    R: Send + 'static,
{
    let handle = std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::error!("[sync] Failed to build tokio runtime: {}", e);
                return None;
            }
        };
        Some(rt.block_on(task()))
    });
    match handle.join() {
        Ok(Some(r)) => Ok(r),
        Ok(None) => Err(SyncHookError::Io(format!(
            "{} failed: could not build runtime",
            description
        ))),
        Err(e) => {
            log::error!("[sync] {} thread panicked: {:?}", description, e);
            Err(SyncHookError::Io(format!(
                "{} thread panicked",
                description
            )))
        }
    }
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

    /// RealTime mode: sync a file to S3 immediately and wait for completion
    fn sync_file_blocking(&self, path: &str) {
        let sync = self.sync_manager.clone();
        let path = path.to_string();
        let _ = run_sync_task("RealTime sync", move || async move {
            if let Err(e) = sync.sync_file_now(&path).await {
                log::error!("[sync] RealTime sync failed for {}: {}", path, e);
            }
        });
    }
}

#[cfg(feature = "s3-sync")]
impl SyncHooks for S3SyncHooks {
    fn on_write(&self, path: &str) {
        if self.sync_manager.is_realtime() {
            self.sync_file_blocking(path);
        } else {
            // Batch mode: enqueue for later (marks the lease dirty when
            // the path is leased; close pushes it).
            self.sync_manager.enqueue_upload(path.to_string());
        }
    }

    fn on_delete(&self, path: &str) {
        self.sync_manager.enqueue_delete(path.to_string());
    }

    fn on_truncate(&self, path: &str) {
        if self.sync_manager.is_realtime() {
            self.sync_file_blocking(path);
        } else {
            self.sync_manager.enqueue_upload(path.to_string());
        }
    }

    fn on_read(&self, path: &str) {
        if self.read_from_s3 {
            // Refresh from S3 before read, waiting for completion
            let sync = self.sync_manager.clone();
            let path = path.to_string();
            let _ = run_sync_task("S3 refresh", move || async move {
                if let Err(e) = sync.refresh_file_from_s3(&path).await {
                    log::error!("[sync] S3 refresh failed for {}: {}", path, e);
                }
            });
        }
    }

    fn on_open(&self, path: &str, kind: OpenKind) -> Result<(), SyncHookError> {
        let sync = self.sync_manager.clone();
        let path_owned = path.to_string();

        if sync.file_lock_enabled() && kind.is_write() {
            // Acquire the lease and refresh the local copy before the
            // local open (which may truncate).
            return run_sync_task("S3 lease acquire", move || async move {
                sync.on_open_write(&path_owned).await
            })?
            .map_err(SyncHookError::from);
        }

        if self.metadata_sync {
            // Check S3 metadata and refresh if changed (like s3fs HEAD
            // request), waiting for completion. Failures are logged only,
            // as before.
            let _ = run_sync_task("S3 metadata check", move || async move {
                match sync.check_and_refresh_from_s3(&path_owned).await {
                    Ok(refreshed) => {
                        if refreshed {
                            log::debug!("[sync] File refreshed on open: {}", path_owned);
                        }
                    }
                    Err(e) => {
                        log::error!("[sync] S3 metadata check failed for {}: {}", path_owned, e);
                    }
                }
            });
        }
        Ok(())
    }

    fn on_close(&self, path: &str, kind: OpenKind) -> Result<(), SyncHookError> {
        if !(self.sync_manager.file_lock_enabled() && kind.is_write()) {
            return Ok(());
        }
        let sync = self.sync_manager.clone();
        let path = path.to_string();
        run_sync_task("S3 lease release", move || async move {
            sync.on_close(&path).await
        })?
        .map_err(SyncHookError::from)
    }

    fn on_fsync(&self, path: &str, kind: OpenKind) -> Result<(), SyncHookError> {
        if !kind.is_write() {
            return Ok(());
        }
        let sync = self.sync_manager.clone();
        let path = path.to_string();
        run_sync_task(
            "S3 fsync",
            move || async move { sync.on_fsync(&path).await },
        )?
        .map_err(SyncHookError::from)
    }

    fn on_mkdir(&self, path: &str) -> Result<(), SyncHookError> {
        let sync = self.sync_manager.clone();
        let path = path.to_string();
        run_sync_task(
            "S3 mkdir",
            move || async move { sync.on_mkdir(&path).await },
        )?
        .map_err(SyncHookError::from)
    }

    fn on_rmdir(&self, path: &str) -> Result<(), SyncHookError> {
        let sync = self.sync_manager.clone();
        let path = path.to_string();
        run_sync_task(
            "S3 rmdir",
            move || async move { sync.on_rmdir(&path).await },
        )?
        .map_err(SyncHookError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_kind_from_fs_flags() {
        assert_eq!(OpenKind::from_fs_flags(fs_core::O_RDONLY), OpenKind::Read);
        assert_eq!(
            OpenKind::from_fs_flags(fs_core::O_RDONLY | fs_core::O_CREAT),
            OpenKind::Read
        );
        assert_eq!(OpenKind::from_fs_flags(fs_core::O_WRONLY), OpenKind::Write);
        assert_eq!(
            OpenKind::from_fs_flags(fs_core::O_RDWR | fs_core::O_TRUNC),
            OpenKind::Write
        );
    }

    #[test]
    fn noop_hooks_accept_every_call() {
        let h = NoOpSyncHooks;
        assert!(h.on_open("/a", OpenKind::Write).is_ok());
        assert!(h.on_close("/a", OpenKind::Write).is_ok());
        assert!(h.on_fsync("/a", OpenKind::Write).is_ok());
        assert!(h.on_mkdir("/d").is_ok());
        assert!(h.on_rmdir("/d").is_ok());
    }
}
