//! Per-file leases: acquisition, contention, expiry takeover, renewal, and
//! close-to-open flushing. Two managers over one `MemoryObjectStore` stand
//! in for two instances sharing a bucket.

mod common;

use std::time::Duration;

use common::*;
use vfs_sync_core::testing::StoreOp;
use vfs_sync_core::{LockRecord, Precondition, SyncError, SyncMode};

fn lock_record(
    store: &vfs_sync_core::testing::MemoryObjectStore,
    path: &str,
) -> Option<LockRecord> {
    store
        .get_raw(&vfs_sync_core::lock_key(path))
        .and_then(|b| LockRecord::parse(&b))
}

#[test]
fn open_write_creates_lock_record_with_instance_and_epoch_one() {
    let store = store();
    let (mgr, _fs) = instance(&store, base_config());
    rt().block_on(async { mgr.on_open_write("/a").await.unwrap() });

    let rec = lock_record(&store, "/a").unwrap();
    assert_eq!(rec.instance, mgr.instance_id());
    assert_eq!(rec.epoch, 1);
    assert_eq!(rec.path, "/a");
    assert!(mgr.is_locked("/a"));
    assert!(store.ops().contains(&StoreOp::Put {
        key: "locks/a".into(),
        cond: Precondition::IfNoneMatchAny,
    }));
}

#[test]
fn open_write_refreshes_local_from_s3_and_close_puts_with_if_match_base() {
    let store = store();
    let remote_etag = store.insert_raw("files/a", b"remote v1");
    let (mgr, fs) = instance(&store, base_config());

    rt().block_on(async { mgr.on_open_write("/a").await.unwrap() });
    assert_eq!(fs.read_local("/a").unwrap(), b"remote v1");

    fs.append_local("/a", b" + local");
    mgr.enqueue_upload("/a".into());
    assert_eq!(mgr.pending_count(), 0, "leased writes are not queued");
    store.clear_ops();

    rt().block_on(async { mgr.on_close("/a").await.unwrap() });

    let ops = store.ops();
    let put_idx = ops
        .iter()
        .position(|op| {
            *op == StoreOp::Put {
                key: "files/a".into(),
                cond: Precondition::IfMatch(remote_etag.clone()),
            }
        })
        .expect("PUT with If-Match base");
    let del_idx = ops
        .iter()
        .position(|op| matches!(op, StoreOp::Delete { key, .. } if key == "locks/a"))
        .expect("lock release");
    assert!(put_idx < del_idx, "data PUT must precede lock release");
    assert_eq!(store.get_raw("files/a").unwrap(), b"remote v1 + local");
    assert!(store.keys_under("locks/").is_empty());
    assert!(!mgr.is_locked("/a"));
}

#[test]
fn close_without_write_does_not_put() {
    let store = store();
    store.insert_raw("files/a", b"remote");
    let (mgr, _fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        store.clear_ops();
        mgr.on_close("/a").await.unwrap();
    });
    assert!(!store
        .ops()
        .iter()
        .any(|op| matches!(op, StoreOp::Put { key, .. } if key == "files/a")));
}

#[test]
fn new_file_is_created_with_if_none_match_on_close() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    rt().block_on(async { mgr.on_open_write("/new").await.unwrap() });
    fs.write_local("/new", b"fresh");
    mgr.enqueue_upload("/new".into());
    rt().block_on(async { mgr.on_close("/new").await.unwrap() });

    assert!(store.ops().contains(&StoreOp::Put {
        key: "files/new".into(),
        cond: Precondition::IfNoneMatchAny,
    }));
    assert_eq!(store.get_raw("files/new").unwrap(), b"fresh");
}

#[test]
fn nested_local_opens_share_one_lease_and_flush_on_last_close() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        mgr.on_open_write("/a").await.unwrap();
    });
    assert_eq!(store.keys_under("locks/").len(), 1);

    fs.write_local("/a", b"x");
    mgr.enqueue_upload("/a".into());

    rt().block_on(async { mgr.on_close("/a").await.unwrap() });
    assert!(mgr.is_locked("/a"));
    assert!(store.get_raw("files/a").is_none());

    rt().block_on(async { mgr.on_close("/a").await.unwrap() });
    assert!(!mgr.is_locked("/a"));
    assert_eq!(store.get_raw("files/a").unwrap(), b"x");
}

#[test]
fn contention_second_instance_gets_busy_after_timeout() {
    let store = store();
    let mut cfg = base_config();
    cfg.lock_timeout = Duration::from_millis(150);
    let ((a, _), (b, _)) = two_instances(&store, cfg);

    rt().block_on(async {
        a.on_open_write("/a").await.unwrap();
        let started = std::time::Instant::now();
        let err = b.on_open_write("/a").await.unwrap_err();
        assert!(started.elapsed() >= Duration::from_millis(150));
        match err {
            SyncError::Busy { path, holder } => {
                assert_eq!(path, "/a");
                assert_eq!(holder.as_deref(), Some(a.instance_id()));
            }
            other => panic!("expected Busy, got {other:?}"),
        }
    });
    assert!(!b.is_locked("/a"));
}

#[test]
fn contention_second_instance_acquires_after_release_and_sees_first_write() {
    let store = store();
    let mut cfg = base_config();
    cfg.lock_timeout = Duration::from_secs(5);
    let ((a, fs_a), (b, fs_b)) = two_instances(&store, cfg);

    rt().block_on(async {
        a.on_open_write("/a").await.unwrap();
        fs_a.write_local("/a", b"from a");
        a.enqueue_upload("/a".into());

        let waiter = async {
            b.on_open_write("/a").await.unwrap();
        };
        let releaser = async {
            tokio::time::sleep(Duration::from_millis(80)).await;
            a.on_close("/a").await.unwrap();
        };
        tokio::join!(waiter, releaser);
    });

    // Close-to-open: B's open observed A's close.
    assert_eq!(fs_b.read_local("/a").unwrap(), b"from a");
    assert!(b.is_locked("/a"));
    assert!(!a.is_locked("/a"));
    assert_eq!(lock_record(&store, "/a").unwrap().instance, b.instance_id());
}

#[test]
fn expired_lease_is_taken_over_with_epoch_increment() {
    let store = store();
    let mut cfg = base_config();
    cfg.lock_lease = Duration::from_millis(40);
    cfg.lock_timeout = Duration::from_millis(500);
    let ((a, _), (b, _)) = two_instances(&store, cfg);

    rt().block_on(async {
        a.on_open_write("/a").await.unwrap();
        // A never renews (no maybe_sync), so its lease lapses.
        tokio::time::sleep(Duration::from_millis(60)).await;
        b.on_open_write("/a").await.unwrap();
    });

    let rec = lock_record(&store, "/a").unwrap();
    assert_eq!(rec.instance, b.instance_id());
    assert_eq!(rec.epoch, 2);
}

#[test]
fn fencing_old_holder_loses_after_new_holder_wrote() {
    let store = store();
    let mut cfg = base_config();
    cfg.lock_lease = Duration::from_millis(40);
    cfg.lock_timeout = Duration::from_millis(500);
    let ((a, fs_a), (b, fs_b)) = two_instances(&store, cfg);

    rt().block_on(async {
        a.on_open_write("/a").await.unwrap();
        fs_a.write_local("/a", b"from a");
        a.enqueue_upload("/a".into());

        tokio::time::sleep(Duration::from_millis(60)).await;
        b.on_open_write("/a").await.unwrap();
        fs_b.write_local("/a", b"from b");
        b.enqueue_upload("/a".into());
        b.on_close("/a").await.unwrap();

        // A's close carries If-None-Match (it saw no object at open) and
        // must be rejected; A's local copy then reflects B's write.
        let err = a.on_close("/a").await.unwrap_err();
        assert!(matches!(
            err,
            SyncError::Conflict {
                refreshed: true,
                ..
            }
        ));
    });

    assert_eq!(store.get_raw("files/a").unwrap(), b"from b");
    assert_eq!(fs_a.read_local("/a").unwrap(), b"from b");
    assert!(!a.is_locked("/a"));
    // A must not have deleted B's lock record (B already released it), and
    // nothing is left behind.
    assert!(store.keys_under("locks/").is_empty());
}

#[test]
fn fencing_gap_old_holder_can_still_win_if_new_holder_has_not_written() {
    // Documents the known window: after A's lease expires and B takes over,
    // A's fenced PUT still succeeds if B has not written yet. B then gets a
    // conflict on its own close. No data is lost either way.
    let store = store();
    let mut cfg = base_config();
    cfg.lock_lease = Duration::from_millis(40);
    cfg.lock_timeout = Duration::from_millis(500);
    let ((a, fs_a), (b, fs_b)) = two_instances(&store, cfg);

    rt().block_on(async {
        a.on_open_write("/a").await.unwrap();
        fs_a.write_local("/a", b"from a");
        a.enqueue_upload("/a".into());

        tokio::time::sleep(Duration::from_millis(60)).await;
        b.on_open_write("/a").await.unwrap();
        fs_b.write_local("/a", b"from b");
        b.enqueue_upload("/a".into());

        a.on_close("/a").await.unwrap();
        let err = b.on_close("/a").await.unwrap_err();
        assert!(err.is_conflict());
    });

    assert_eq!(store.get_raw("files/a").unwrap(), b"from a");
    assert_eq!(fs_b.read_local("/a").unwrap(), b"from a");
}

#[test]
fn renewal_extends_expiry_and_lost_renewal_marks_lease() {
    let store = store();
    let mut cfg = base_config();
    cfg.lock_lease = Duration::from_millis(100);
    cfg.poll_interval = Duration::from_secs(3600);
    let (mgr, _fs) = instance(&store, cfg);

    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        let first = lock_record(&store, "/a").unwrap();

        tokio::time::sleep(Duration::from_millis(60)).await;
        mgr.maybe_sync().await;
        let renewed = lock_record(&store, "/a").unwrap();
        assert!(renewed.expires_ms > first.expires_ms);
        assert_eq!(renewed.epoch, first.epoch);

        // Another instance overwrote the record; the next renewal loses.
        store.insert_raw(
            &vfs_sync_core::lock_key("/a"),
            b"instance=other\nepoch=9\nexpires=99999999999999\npath=/a\n",
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        mgr.maybe_sync().await;

        // The lease is still tracked locally, and close must not delete the
        // foreign record.
        assert!(mgr.is_locked("/a"));
        let _ = mgr.on_close("/a").await;
    });
    assert_eq!(lock_record(&store, "/a").unwrap().instance, "other");
}

#[test]
fn pending_queued_upload_is_pushed_before_lock_refresh() {
    let store = store();
    let mut cfg = base_config();
    cfg.flush_interval = Duration::from_secs(3600);
    let (mgr, fs) = instance(&store, cfg);

    fs.write_local("/a", b"queued");
    mgr.enqueue_upload("/a".into());
    assert_eq!(mgr.pending_count(), 1);

    rt().block_on(async { mgr.on_open_write("/a").await.unwrap() });

    assert_eq!(mgr.pending_count(), 0);
    assert_eq!(store.get_raw("files/a").unwrap(), b"queued");
    assert_eq!(fs.read_local("/a").unwrap(), b"queued");
}

#[test]
fn unlink_while_locked_deletes_on_close_with_if_match() {
    let store = store();
    let remote_etag = store.insert_raw("files/a", b"remote");
    let (mgr, fs) = instance(&store, base_config());

    rt().block_on(async { mgr.on_open_write("/a").await.unwrap() });
    fs.remove_local("/a");
    mgr.enqueue_delete("/a".into());
    assert_eq!(mgr.pending_count(), 0);
    rt().block_on(async { mgr.on_close("/a").await.unwrap() });

    assert!(store.ops().contains(&StoreOp::Delete {
        key: "files/a".into(),
        cond: Precondition::IfMatch(remote_etag),
    }));
    assert!(store.get_raw("files/a").is_none());
    assert!(store.keys_under("locks/").is_empty());
}

#[test]
fn realtime_write_under_lock_puts_immediately_and_advances_base() {
    let store = store();
    let mut cfg = base_config();
    cfg.mode = SyncMode::RealTime;
    let (mgr, fs) = instance(&store, cfg);

    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        fs.write_local("/a", b"1");
        mgr.sync_file_now("/a").await.unwrap();
        let first = store.etag_of("files/a").unwrap();
        store.clear_ops();

        fs.write_local("/a", b"12");
        mgr.sync_file_now("/a").await.unwrap();
        assert!(store.ops().contains(&StoreOp::Put {
            key: "files/a".into(),
            cond: Precondition::IfMatch(first),
        }));
        store.clear_ops();

        mgr.on_close("/a").await.unwrap();
    });
    // Nothing was dirty at close, so no further PUT.
    assert!(!store
        .ops()
        .iter()
        .any(|op| matches!(op, StoreOp::Put { key, .. } if key == "files/a")));
    assert_eq!(store.get_raw("files/a").unwrap(), b"12");
}

#[test]
fn fsync_flushes_without_releasing_lease() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        fs.write_local("/a", b"partial");
        mgr.enqueue_upload("/a".into());
        mgr.on_fsync("/a").await.unwrap();
    });
    assert_eq!(store.get_raw("files/a").unwrap(), b"partial");
    assert!(mgr.is_locked("/a"));
    assert_eq!(store.keys_under("locks/").len(), 1);
}

#[test]
fn release_all_leases_flushes_dirty_paths_and_removes_lock_objects() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        mgr.on_open_write("/b").await.unwrap();
        fs.write_local("/a", b"a");
        mgr.enqueue_upload("/a".into());
        mgr.release_all_leases().await.unwrap();
    });
    assert_eq!(store.get_raw("files/a").unwrap(), b"a");
    assert!(store.get_raw("files/b").is_none());
    assert!(store.keys_under("locks/").is_empty());
    assert!(!mgr.is_locked("/a"));
    assert!(!mgr.is_locked("/b"));
}

#[test]
fn file_lock_disabled_makes_lease_calls_noops() {
    let store = store();
    let mut cfg = base_config();
    cfg.file_lock = false;
    let (mgr, _fs) = instance(&store, cfg);
    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        mgr.on_close("/a").await.unwrap();
    });
    assert!(store.keys_under("locks/").is_empty());
    assert!(!mgr.is_locked("/a"));
}

#[test]
fn read_through_refresh_is_noop_for_leased_path() {
    let store = store();
    store.insert_raw("files/a", b"v1");
    let (mgr, fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
        fs.write_local("/a", b"local");
        store.insert_raw("files/a", b"v2");
        mgr.refresh_file_from_s3("/a").await.unwrap();
        assert!(!mgr.check_and_refresh_from_s3("/a").await.unwrap());
    });
    assert_eq!(fs.read_local("/a").unwrap(), b"local");
}

#[test]
fn append_from_two_instances_is_serialised_by_the_lease() {
    // The s3-sync-logging shape: two instances append to one log. With the
    // lease, each open sees the other's close, so no line is lost.
    let store = store();
    let mut cfg = base_config();
    cfg.lock_timeout = Duration::from_secs(5);
    let ((a, fs_a), (b, fs_b)) = two_instances(&store, cfg);

    rt().block_on(async {
        for i in 0..3 {
            a.on_open_write("/log").await.unwrap();
            fs_a.append_local("/log", format!("a{i}\n").as_bytes());
            a.enqueue_upload("/log".into());
            a.on_close("/log").await.unwrap();

            b.on_open_write("/log").await.unwrap();
            fs_b.append_local("/log", format!("b{i}\n").as_bytes());
            b.enqueue_upload("/log".into());
            b.on_close("/log").await.unwrap();
        }
    });

    assert_eq!(
        store.get_raw("files/log").unwrap(),
        b"a0\nb0\na1\nb1\na2\nb2\n"
    );
    assert!(store.keys_under("locks/").is_empty());
}
