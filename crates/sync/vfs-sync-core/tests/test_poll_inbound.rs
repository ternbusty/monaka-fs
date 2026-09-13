//! Inbound polling decides by ETag only and never touches leased paths.

mod common;

use common::*;

fn sync(mgr: &Mgr) {
    rt().block_on(async {
        mgr.maybe_sync().await;
    });
}

#[test]
fn poll_downloads_when_etag_differs_even_if_remote_looks_older() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });

    // A foreign write with a last-modified stamp lower than anything this
    // instance has seen. The old wall-clock rule would have skipped it.
    store.insert_raw("files/a", b"foreign");
    store.set_last_modified("files/a", 0);

    sync(&mgr);
    assert_eq!(fs.read_local("/a").unwrap(), b"foreign");
}

#[test]
fn poll_skips_when_etag_matches_even_if_remote_looks_newer() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    fs.write_local("/a", b"one");
    mgr.enqueue_upload("/a".into());
    rt().block_on(async { mgr.force_flush().await.unwrap() });
    store.set_last_modified("files/a", u64::MAX);
    store.clear_ops();

    sync(&mgr);
    assert!(!store.ops().iter().any(|op| matches!(
        op,
        vfs_sync_core::testing::StoreOp::Get { key } if key == "files/a"
    )));
}

#[test]
fn poll_downloads_unknown_paths_and_creates_parents() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    store.insert_raw("files/dir/new.txt", b"hello");

    sync(&mgr);
    assert_eq!(fs.read_local("/dir/new.txt").unwrap(), b"hello");
    assert!(fs.has_dir("/dir"));
}

#[test]
fn poll_skips_paths_with_pending_outbound_ops() {
    let store = store();
    let mut cfg = base_config();
    cfg.outbound_batch_size = 100;
    cfg.flush_interval = std::time::Duration::from_secs(3600);
    let (mgr, fs) = instance(&store, cfg);

    store.insert_raw("files/a", b"remote");
    fs.write_local("/a", b"local");
    mgr.enqueue_upload("/a".into());

    sync(&mgr);
    assert_eq!(fs.read_local("/a").unwrap(), b"local");
}

#[test]
fn poll_skips_paths_leased_by_this_instance() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    store.insert_raw("files/a", b"v1");

    rt().block_on(async {
        mgr.on_open_write("/a").await.unwrap();
    });
    assert_eq!(fs.read_local("/a").unwrap(), b"v1");
    fs.write_local("/a", b"local edit");
    mgr.enqueue_upload("/a".into());

    store.insert_raw("files/a", b"v2");
    sync(&mgr);
    assert_eq!(fs.read_local("/a").unwrap(), b"local edit");
}

#[test]
fn poll_deletes_local_file_missing_remotely() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    store.insert_raw("files/a", b"one");
    sync(&mgr);
    assert!(fs.read_local("/a").is_some());

    store.remove_raw("files/a");
    sync(&mgr);
    assert!(fs.read_local("/a").is_none());
}

#[test]
fn poll_does_not_delete_leased_path_missing_remotely() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_open_write("/new").await.unwrap();
    });
    fs.write_local("/new", b"draft");
    mgr.enqueue_upload("/new".into());

    sync(&mgr);
    assert_eq!(fs.read_local("/new").unwrap(), b"draft");
}

#[test]
fn poll_ignores_lock_objects() {
    let store = store();
    let (mgr, fs) = instance(&store, base_config());
    store.insert_raw("locks/a", b"instance=x\nepoch=1\nexpires=1\n");

    sync(&mgr);
    assert!(fs.file_paths().is_empty());
}

#[test]
fn two_instances_converge_through_polling() {
    let store = store();
    let ((a, fs_a), (b, fs_b)) = two_instances(&store, base_config());

    fs_a.write_local("/shared", b"from a");
    a.enqueue_upload("/shared".into());
    rt().block_on(async { a.force_flush().await.unwrap() });

    sync(&b);
    assert_eq!(fs_b.read_local("/shared").unwrap(), b"from a");
}
