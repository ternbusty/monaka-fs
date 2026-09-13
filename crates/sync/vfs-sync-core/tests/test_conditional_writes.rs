//! Conditional PUT / DELETE behaviour of the outbound path, with file
//! locks disabled so the queue path (not the lease path) is exercised.

mod common;

use common::*;
use vfs_sync_core::testing::StoreOp;
use vfs_sync_core::{Precondition, SyncError, SyncMode};

fn unlocked() -> vfs_sync_core::SyncConfig {
    let mut c = base_config();
    c.file_lock = false;
    c
}

fn puts(store: &vfs_sync_core::testing::MemoryObjectStore) -> Vec<Precondition> {
    store
        .ops()
        .into_iter()
        .filter_map(|op| match op {
            StoreOp::Put { key, cond } if key.starts_with("files/") => Some(cond),
            _ => None,
        })
        .collect()
}

#[test]
fn first_upload_uses_if_none_match_and_pins_etag() {
    let store = store();
    let (mgr, fs) = instance(&store, unlocked());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());

    rt().block_on(async {
        assert_eq!(mgr.force_flush().await.unwrap(), 1);
    });

    assert_eq!(puts(&store), vec![Precondition::IfNoneMatchAny]);
    assert_eq!(store.get_raw("files/a").unwrap(), b"one");
}

#[test]
fn second_upload_uses_if_match_previous_etag() {
    let store = store();
    let (mgr, fs) = instance(&store, unlocked());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });
    let first = store.etag_of("files/a").unwrap();
    store.clear_ops();

    fs.write_local("/a", b"two");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    assert_eq!(puts(&store), vec![Precondition::IfMatch(first)]);
    assert_eq!(store.get_raw("files/a").unwrap(), b"two");
}

#[test]
fn lock_disabled_conflict_falls_back_to_last_writer_wins() {
    let store = store();
    let (mgr, fs) = instance(&store, unlocked());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    // Someone else replaced the object behind our back.
    store.insert_raw("files/a", b"foreign");
    store.clear_ops();

    fs.write_local("/a", b"two");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    // Conditional attempt, then an unconditional retry.
    let conds = puts(&store);
    assert!(matches!(conds[0], Precondition::IfMatch(_)));
    assert_eq!(conds[1], Precondition::None);
    assert_eq!(store.get_raw("files/a").unwrap(), b"two");
    assert_eq!(mgr.pending_count(), 0);
}

#[test]
fn lock_enabled_conflict_keeps_remote_and_refreshes_local() {
    // File lock enabled but the path was never opened for write through the
    // manager (no lease), so the queue path resolves the conflict by
    // adopting the remote version.
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    store.insert_raw("files/a", b"foreign");

    fs.write_local("/a", b"two");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    assert_eq!(store.get_raw("files/a").unwrap(), b"foreign");
    assert_eq!(fs.read_local("/a").unwrap(), b"foreign");
    assert_eq!(mgr.pending_count(), 0);
}

#[test]
fn realtime_sync_file_now_is_conditional_and_reports_conflict() {
    let store = store();
    let mut cfg = base_config();
    cfg.mode = SyncMode::RealTime;
    let (mgr, fs) = instance(&store, cfg);

    fs.write_local("/a", b"one");
    rt().block_on(async { mgr.sync_file_now("/a").await.unwrap() });
    store.insert_raw("files/a", b"foreign");

    fs.write_local("/a", b"two");
    let err = rt()
        .block_on(async { mgr.sync_file_now("/a").await })
        .unwrap_err();
    assert!(matches!(
        err,
        SyncError::Conflict {
            refreshed: true,
            ..
        }
    ));
    assert_eq!(fs.read_local("/a").unwrap(), b"foreign");
}

#[test]
fn upload_after_concurrent_delete_recreates_object() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    store.remove_raw("files/a");
    store.clear_ops();

    fs.write_local("/a", b"two");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    let conds = puts(&store);
    assert!(matches!(conds[0], Precondition::IfMatch(_)));
    assert_eq!(conds[1], Precondition::IfNoneMatchAny);
    assert_eq!(store.get_raw("files/a").unwrap(), b"two");
}

#[test]
fn delete_uses_if_match_with_last_known_etag() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });
    let etag = store.etag_of("files/a").unwrap();
    store.clear_ops();

    fs.remove_local("/a");
    mgr.enqueue_delete("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    assert!(store.ops().contains(&StoreOp::Delete {
        key: "files/a".into(),
        cond: Precondition::IfMatch(etag),
    }));
    assert!(store.get_raw("files/a").is_none());
}

#[test]
fn delete_conflict_restores_remote_locally_when_locked() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    store.insert_raw("files/a", b"foreign");

    fs.remove_local("/a");
    mgr.enqueue_delete("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    // The zombie problem in reverse: our stale delete must not remove the
    // foreign version, and the local FS should learn about it.
    assert_eq!(store.get_raw("files/a").unwrap(), b"foreign");
    assert_eq!(fs.read_local("/a").unwrap(), b"foreign");
}

#[test]
fn delete_conflict_deletes_anyway_when_unlocked() {
    let store = store();
    let (mgr, fs) = instance(&store, unlocked());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    store.insert_raw("files/a", b"foreign");

    fs.remove_local("/a");
    mgr.enqueue_delete("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    assert!(store.get_raw("files/a").is_none());
}

#[test]
fn delete_falls_back_when_store_rejects_conditional_delete() {
    let store = store();
    store.set_reject_conditional_delete(true);
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    fs.remove_local("/a");
    mgr.enqueue_delete("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    assert!(store.get_raw("files/a").is_none());
}

#[test]
fn transient_upload_error_requeues_and_returns_error() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    store.fail_next(vfs_sync_core::S3Error::Write {
        key: "files/a".into(),
        message: "boom".into(),
    });

    let err = rt().block_on(async { mgr.force_flush().await });
    assert!(err.is_err());
    assert_eq!(mgr.pending_count(), 1);

    rt().block_on(async { mgr.force_flush().await.unwrap() });
    assert_eq!(store.get_raw("files/a").unwrap(), b"one");
}
