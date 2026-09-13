//! S3 synchronization module for vfs-adapter
//!
//! Thin wrapper around vfs-sync-adapter for S3 sync capabilities. Every
//! hook runs on the adapter's own current-thread tokio runtime via
//! `block_on`; none of them may be called while that runtime is already
//! running (nothing in the adapter does so today).

use std::cell::RefCell;
use std::rc::Rc;

use std::sync::Arc;

use vfs_sync_adapter::{
    new_s3_storage, AdapterFs, MetadataCache, SyncConfig, SyncError, SyncManager,
};

use crate::exports::wasi::filesystem::types::ErrorCode;
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

fn is_write_access(flags: u32) -> bool {
    flags & 0x3 != fs_core::O_RDONLY
}

/// Map a sync error to the WASI error code the application sees.
fn map_sync_error(e: SyncError) -> ErrorCode {
    match e {
        SyncError::Busy { .. } => ErrorCode::Busy,
        SyncError::Conflict { .. } => ErrorCode::NotRecoverable,
        other => {
            log::error!("[s3-sync] {}", other);
            ErrorCode::Io
        }
    }
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
        log::info!(
            "[s3-sync] file lock: {}",
            if config.file_lock {
                "enabled"
            } else {
                "disabled"
            }
        );
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
            // Batch mode: enqueue for later (marks the lease dirty when
            // the path is leased; close pushes it).
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

/// Called before a file is opened with fs-core `flags`. For write opens
/// with the file lock enabled this acquires the S3 lease and refreshes the
/// local copy; a `Busy` or `NotRecoverable` error aborts the open.
/// Otherwise, with `VFS_METADATA_MODE=s3`, it performs the s3fs-style HEAD
/// check.
pub fn on_open(path: &str, flags: u32) -> Result<(), ErrorCode> {
    with_sync_state(|state, sync| {
        if sync.file_lock_enabled() && is_write_access(flags) {
            return state
                .runtime
                .block_on(async { sync.on_open_write(path).await })
                .map_err(map_sync_error);
        }

        if std::env::var("VFS_METADATA_MODE").unwrap_or_default() == "s3" {
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
        Ok(())
    })
    .unwrap_or(Ok(()))
}

/// Called when a descriptor is dropped. Write descriptors push their
/// pending content and release the lease.
pub fn on_close(path: &str, flags: u32) -> Result<(), ErrorCode> {
    if !is_write_access(flags) {
        return Ok(());
    }
    with_sync_state(|state, sync| {
        if !sync.file_lock_enabled() {
            return Ok(());
        }
        state
            .runtime
            .block_on(async { sync.on_close(path).await })
            .map_err(map_sync_error)
    })
    .unwrap_or(Ok(()))
}

/// `sync` / `sync-data`: push a write descriptor's content to S3 without
/// releasing its lease. This is the only call that can report a lease
/// conflict to the application, since descriptor drop cannot fail.
pub fn on_fsync(path: &str, flags: u32) -> Result<(), ErrorCode> {
    if !is_write_access(flags) {
        return Ok(());
    }
    with_sync_state(|state, sync| {
        // The stream's dirty flag only reaches the manager on stream
        // drop; register the write explicitly so fsync has something to
        // push.
        sync.enqueue_upload(path.to_string());
        state
            .runtime
            .block_on(async { sync.on_fsync(path).await })
            .map_err(map_sync_error)
    })
    .unwrap_or(Ok(()))
}

/// Directory marker creation. Failures are logged; the local directory
/// exists regardless.
pub fn on_mkdir(path: &str) {
    with_sync_state(|state, sync| {
        state.runtime.block_on(async {
            if let Err(e) = sync.on_mkdir(path).await {
                log::error!("[s3-sync] mkdir marker for {} failed: {}", path, e);
            }
        });
    });
}

/// Directory marker removal. Failures are logged.
pub fn on_rmdir(path: &str) {
    with_sync_state(|state, sync| {
        state.runtime.block_on(async {
            if let Err(e) = sync.on_rmdir(path).await {
                log::error!("[s3-sync] rmdir marker for {} failed: {}", path, e);
            }
        });
    });
}
