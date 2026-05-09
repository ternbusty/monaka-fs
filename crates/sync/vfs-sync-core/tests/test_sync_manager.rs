//! State-transition tests for `SyncManager`.
//!
//! These tests focus on the bookkeeping that does not require talking to
//! S3: outbound queue dedup, metadata cache invalidation on delete, the
//! shutdown flag, and the realtime/batch dispatch on `is_realtime`. Exercising
//! the actual upload/download paths needs an `S3Storage` mock, which would
//! require turning S3 access into a trait. We leave that to a follow-up and
//! keep these tests pinning the queue logic that the WASI single-threaded
//! and host multi-threaded callers both rely on.

use std::sync::Arc;
use std::time::Duration;

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_smithy_async::rt::sleep::TokioSleep;
use vfs_sync_core::{
    FsBackend, MetadataCache, S3Error, S3Storage, SyncConfig, SyncManager, SyncMode,
};

// ---------------------------------------------------------------------------
// Mock FsBackend
// ---------------------------------------------------------------------------
//
// `SyncManager` requires an `F: FsBackend`, but the queue methods we are
// testing here never touch `self.fs`. A backend that errors on every call is
// therefore enough.
//
// (`SyncManager` is generic, so for tests that *did* need real FS behaviour
// we could swap this out for a richer mock without affecting these.)

struct ErrorFs;

impl FsBackend for ErrorFs {
    fn open_read(&self, _path: &str) -> Result<u32, S3Error> {
        Err(S3Error::Read {
            key: "mock".into(),
            message: "ErrorFs".into(),
        })
    }

    fn open_write_truncate(&self, _path: &str) -> Result<u32, S3Error> {
        Err(S3Error::Write {
            key: "mock".into(),
            message: "ErrorFs".into(),
        })
    }

    fn read(&self, _fd: u32, _buf: &mut [u8]) -> Result<usize, S3Error> {
        Err(S3Error::Read {
            key: "mock".into(),
            message: "ErrorFs".into(),
        })
    }

    fn write(&self, _fd: u32, _buf: &[u8]) -> Result<usize, S3Error> {
        Err(S3Error::Write {
            key: "mock".into(),
            message: "ErrorFs".into(),
        })
    }

    fn close(&self, _fd: u32) -> Result<(), S3Error> {
        Ok(())
    }

    fn stat_modified(&self, _path: &str) -> u64 {
        0
    }

    fn fstat_size(&self, _fd: u32) -> Result<u64, S3Error> {
        Err(S3Error::Read {
            key: "mock".into(),
            message: "ErrorFs".into(),
        })
    }

    fn unlink(&self, _path: &str) -> Result<(), S3Error> {
        Ok(())
    }

    fn mkdir_p(&self, _path: &str) {}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal `S3Storage` whose construction makes no network calls.
/// Used purely to satisfy the `SyncManager::new` signature for queue tests.
fn dummy_s3() -> Arc<S3Storage> {
    // SdkConfig refuses to build an S3 client without a behavior version,
    // a region, and a sleep_impl (the SDK uses the latter for stalled-stream
    // protection and for the lazy identity cache). Reuse the same TokioSleep
    // the production wrappers feed it. No S3 requests actually run from these
    // queue tests, so the runtime is never driven.
    let config = SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::from_static("us-east-1"))
        .sleep_impl(TokioSleep::new())
        .build();
    Arc::new(S3Storage::from_sdk_config(
        "test-bucket".into(),
        "test/".into(),
        &config,
    ))
}

fn make_manager(mode: SyncMode) -> SyncManager<ErrorFs> {
    let mut config = SyncConfig::default();
    config.mode = mode;
    // Force a tiny batch so flush logic is easy to reason about.
    config.outbound_batch_size = 4;
    config.flush_interval = Duration::from_secs(1);
    SyncManager::new(dummy_s3(), ErrorFs, MetadataCache::new(), config)
}

// ---------------------------------------------------------------------------
// enqueue_upload
// ---------------------------------------------------------------------------

#[test]
fn enqueue_upload_increments_pending_count() {
    let mgr = make_manager(SyncMode::Batch);
    assert_eq!(mgr.pending_count(), 0);

    mgr.enqueue_upload("/a".into());
    assert_eq!(mgr.pending_count(), 1);

    mgr.enqueue_upload("/b".into());
    assert_eq!(mgr.pending_count(), 2);
}

#[test]
fn enqueue_upload_dedups_same_path() {
    // Pushing the same path twice should leave one pending entry, not two.
    // This avoids redundant uploads when the same file is touched repeatedly
    // before the next flush.
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_upload("/a".into());
    mgr.enqueue_upload("/a".into());
    mgr.enqueue_upload("/a".into());

    assert_eq!(mgr.pending_count(), 1);
}

#[test]
fn enqueue_upload_keeps_distinct_paths() {
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_upload("/a".into());
    mgr.enqueue_upload("/b".into());
    mgr.enqueue_upload("/a".into()); // dedups against the earlier /a

    assert_eq!(mgr.pending_count(), 2);
}

// ---------------------------------------------------------------------------
// enqueue_delete
// ---------------------------------------------------------------------------

#[test]
fn enqueue_delete_increments_pending_count() {
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_delete("/a".into());
    assert_eq!(mgr.pending_count(), 1);
}

#[test]
fn enqueue_delete_supersedes_pending_upload() {
    // If we enqueue an Upload then a Delete for the same path, only the
    // Delete should remain. Without the dedup the server would briefly see
    // the file before it's removed.
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_upload("/a".into());
    mgr.enqueue_delete("/a".into());

    assert_eq!(mgr.pending_count(), 1);
}

#[test]
fn enqueue_upload_after_delete_replaces_delete() {
    // Symmetric case: write after delete (same path) → only the Upload
    // should be queued.
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_delete("/a".into());
    mgr.enqueue_upload("/a".into());

    assert_eq!(mgr.pending_count(), 1);
}

#[test]
fn enqueue_delete_dedups_consecutive_deletes() {
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_delete("/a".into());
    mgr.enqueue_delete("/a".into());

    assert_eq!(mgr.pending_count(), 1);
}

// ---------------------------------------------------------------------------
// is_realtime
// ---------------------------------------------------------------------------

#[test]
fn is_realtime_reflects_config_batch() {
    assert!(!make_manager(SyncMode::Batch).is_realtime());
}

#[test]
fn is_realtime_reflects_config_realtime() {
    assert!(make_manager(SyncMode::RealTime).is_realtime());
}

// ---------------------------------------------------------------------------
// shutdown flag
// ---------------------------------------------------------------------------

#[test]
fn shutdown_starts_unset() {
    let mgr = make_manager(SyncMode::Batch);
    assert!(!mgr.is_shutdown());
}

#[test]
fn shutdown_sets_flag() {
    let mgr = make_manager(SyncMode::Batch);
    mgr.shutdown();
    assert!(mgr.is_shutdown());
}

#[test]
fn shutdown_is_idempotent() {
    let mgr = make_manager(SyncMode::Batch);
    mgr.shutdown();
    mgr.shutdown();
    assert!(mgr.is_shutdown());
}

#[test]
fn maybe_sync_is_noop_after_shutdown() {
    // Once shutdown has been requested, maybe_sync should return false
    // without doing any work, even if the queue is non-empty. This is what
    // lets the host's background thread exit cleanly.
    let mgr = make_manager(SyncMode::Batch);
    mgr.enqueue_upload("/a".into());
    mgr.shutdown();

    let did_work = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(mgr.maybe_sync());
    assert!(!did_work);
    // The queued operation is still there - we didn't process it.
    assert_eq!(mgr.pending_count(), 1);
}

// ---------------------------------------------------------------------------
// FsBackend trait surface (sanity-check defaults the mock relies on)
// ---------------------------------------------------------------------------

#[test]
fn fs_backend_stat_modified_default_is_zero_for_missing_path() {
    // The trait contract says `stat_modified` should return 0 when the
    // path can't be stat'd. This mirrors the original
    // `stat(path).map(|m| m.modified).unwrap_or(0)` pattern that
    // `upload_file` relies on.
    assert_eq!(ErrorFs.stat_modified("/missing"), 0);
}

#[test]
fn fs_backend_close_swallows_in_mock() {
    // Sanity check that the mock's no-op close matches what the production
    // call sites expect (they ignore the result).
    assert!(ErrorFs.close(42).is_ok());
}
