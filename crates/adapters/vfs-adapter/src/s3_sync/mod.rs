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
    with_sync_manager(|sync| {
        sync.enqueue_upload(path.to_string());
    });

    // Always try to sync after write (will respect batch/realtime config)
    maybe_sync();
}

/// Notify sync manager of a file deletion
pub fn on_delete(path: &str) {
    with_sync_manager(|sync| {
        sync.enqueue_delete(path.to_string());
    });

    // Always try to sync after delete
    maybe_sync();
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
