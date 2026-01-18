//! S3 synchronization module for vfs-adapter
//!
//! Thin wrapper around vfs-sync-wasi for S3 sync capabilities.

use std::cell::RefCell;
use std::rc::Rc;

use vfs_sync_wasi::{MetadataCache, S3Storage, SyncConfig, SyncManager};

use crate::Fs;
use crate::SystemTimeProvider;

/// Global sync manager state
static mut SYNC_STATE: Option<SyncState> = None;

struct SyncState {
    sync_manager: Rc<RefCell<Option<SyncManager<SystemTimeProvider>>>>,
    runtime: tokio::runtime::Runtime,
}

/// Initialize S3 sync from environment variables
/// Called during adapter initialization
pub fn init_s3_sync(fs: Rc<RefCell<Fs<SystemTimeProvider>>>) {
    // Check if S3 sync is enabled via environment variable
    let bucket = match std::env::var("VFS_S3_BUCKET") {
        Ok(b) if !b.is_empty() => b,
        _ => return,
    };

    let prefix = std::env::var("VFS_S3_PREFIX").unwrap_or_else(|_| "vfs/".to_string());

    // Create tokio runtime for async operations
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    // Initialize S3 client and sync manager
    let sync_manager = runtime.block_on(async {
        let s3 = S3Storage::new(bucket, prefix).await;
        let s3 = Rc::new(s3);
        let config = SyncConfig::from_env();
        let cache = MetadataCache::new();

        SyncManager::new(s3, fs, cache, config)
    });

    unsafe {
        SYNC_STATE = Some(SyncState {
            sync_manager: Rc::new(RefCell::new(Some(sync_manager))),
            runtime,
        });
    }
}

/// Notify sync manager of a file write
pub fn on_write(path: &str) {
    unsafe {
        if let Some(ref mut state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                if sync.is_realtime() {
                    // RealTime mode: sync immediately and block until S3 upload completes
                    state.runtime.block_on(async {
                        if let Err(e) = sync.sync_file_now(path).await {
                            log::error!("[s3-sync] realtime sync failed for {}: {}", path, e);
                        }
                    });
                } else {
                    // Batch mode: enqueue for later
                    sync.enqueue_upload(path.to_string());
                    // Try to flush if batch is ready
                    state.runtime.block_on(async {
                        let _ = sync.maybe_sync().await;
                    });
                }
            }
        }
    }
}

/// Notify sync manager of a file deletion
pub fn on_delete(path: &str) {
    unsafe {
        if let Some(ref mut state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                // Enqueue delete operation
                sync.enqueue_delete(path.to_string());
                // Flush immediately in realtime mode
                state.runtime.block_on(async {
                    let _ = sync.maybe_sync().await;
                });
            }
        }
    }
}

/// Refresh file from S3 before read (if VFS_READ_MODE=s3)
/// Called when a file is read to ensure we have the latest S3 content
pub fn on_read(path: &str) {
    // Check if read-through mode is enabled
    if std::env::var("VFS_READ_MODE").unwrap_or_default() != "s3" {
        return;
    }

    unsafe {
        if let Some(ref mut state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                state.runtime.block_on(async {
                    if let Err(e) = sync.refresh_file_from_s3(path).await {
                        log::error!("[s3-sync] refresh failed for {}: {}", path, e);
                    }
                });
            }
        }
    }
}

/// Check S3 metadata and refresh if changed (if VFS_METADATA_MODE=s3)
/// Called when a file is opened to ensure metadata is fresh (like s3fs HEAD request)
pub fn on_open(path: &str) {
    // Check if metadata sync mode is enabled
    if std::env::var("VFS_METADATA_MODE").unwrap_or_default() != "s3" {
        return;
    }

    unsafe {
        if let Some(ref mut state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                state.runtime.block_on(async {
                    match sync.check_and_refresh_from_s3(path).await {
                        Ok(refreshed) => {
                            if refreshed {
                                log::debug!("[s3-sync] refreshed on open: {}", path);
                            }
                        }
                        Err(e) => {
                            log::error!("[s3-sync] metadata check failed for {}: {}", path, e);
                        }
                    }
                });
            }
        }
    }
}

/// Run pending sync operations
/// Called periodically from the main loop
pub fn maybe_sync() {
    unsafe {
        if let Some(ref mut state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                state.runtime.block_on(async {
                    let _ = sync.maybe_sync().await;
                });
            }
        }
    }
}

/// Force flush all pending sync operations
#[allow(dead_code)]
pub fn force_flush() {
    unsafe {
        if let Some(ref mut state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                state.runtime.block_on(async {
                    let _ = sync.force_flush().await;
                });
            }
        }
    }
}

fn with_sync_manager<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&SyncManager<SystemTimeProvider>) -> R,
{
    unsafe {
        if let Some(ref state) = SYNC_STATE {
            if let Some(ref sync) = *state.sync_manager.borrow() {
                return Some(f(sync));
            }
        }
    }
    None
}
