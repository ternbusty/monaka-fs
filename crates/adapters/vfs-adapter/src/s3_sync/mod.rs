//! S3 synchronization module for vfs-adapter
//!
//! Thin wrapper around vfs-sync-adapter for S3 sync capabilities.

use std::cell::RefCell;
use std::rc::Rc;

use std::sync::Arc;

use vfs_sync_adapter::{new_s3_storage, AdapterFs, MetadataCache, SyncConfig, SyncManager};

use crate::Fs;
use crate::SystemTimeProvider;

struct SyncState {
    sync_manager: Rc<RefCell<Option<SyncManager<SystemTimeProvider>>>>,
    runtime: tokio::runtime::Runtime,
}

thread_local! {
    static SYNC_STATE: RefCell<Option<SyncState>> = const { RefCell::new(None) };
}

fn with_sync_state<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&SyncState, &SyncManager<SystemTimeProvider>) -> R,
{
    SYNC_STATE.with(|cell| {
        let state_ref = cell.borrow();
        let state = state_ref.as_ref()?;
        let sync_borrow = state.sync_manager.borrow();
        let sync = sync_borrow.as_ref()?;
        Some(f(state, sync))
    })
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
        let s3 = new_s3_storage(bucket, prefix).await;
        let s3 = Arc::new(s3);
        let config = SyncConfig::from_env();
        let cache = MetadataCache::new();

        SyncManager::new(s3, AdapterFs(fs), cache, config)
    });

    SYNC_STATE.with(|cell| {
        *cell.borrow_mut() = Some(SyncState {
            sync_manager: Rc::new(RefCell::new(Some(sync_manager))),
            runtime,
        });
    });
}

/// Notify sync manager of a file write
pub fn on_write(path: &str) {
    with_sync_state(|state, sync| {
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
    });
}

/// Notify sync manager of a file deletion
pub fn on_delete(path: &str) {
    with_sync_state(|state, sync| {
        // Enqueue delete operation
        sync.enqueue_delete(path.to_string());
        // Flush immediately in realtime mode
        state.runtime.block_on(async {
            let _ = sync.maybe_sync().await;
        });
    });
}

/// Refresh file from S3 before read (if VFS_READ_MODE=s3)
/// Called when a file is read to ensure we have the latest S3 content
pub fn on_read(path: &str) {
    // Check if read-through mode is enabled
    if std::env::var("VFS_READ_MODE").unwrap_or_default() != "s3" {
        return;
    }

    with_sync_state(|state, sync| {
        state.runtime.block_on(async {
            if let Err(e) = sync.refresh_file_from_s3(path).await {
                log::error!("[s3-sync] refresh failed for {}: {}", path, e);
            }
        });
    });
}

/// Check S3 metadata and refresh if changed (if VFS_METADATA_MODE=s3)
/// Called when a file is opened to ensure metadata is fresh (like s3fs HEAD request)
pub fn on_open(path: &str) {
    // Check if metadata sync mode is enabled
    if std::env::var("VFS_METADATA_MODE").unwrap_or_default() != "s3" {
        return;
    }

    with_sync_state(|state, sync| {
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
    });
}
