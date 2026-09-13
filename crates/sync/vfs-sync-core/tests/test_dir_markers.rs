//! Directory markers: `files/<dir>/` objects created on mkdir, removed on
//! rmdir, and materialised on the other side by populate and poll.

mod common;

use common::*;
use vfs_sync_core::testing::StoreOp;
use vfs_sync_core::{populate_from_s3, Precondition};

#[test]
fn on_mkdir_puts_marker_with_if_none_match() {
    let store = store();
    let (mgr, _fs) = instance(&store, base_config());
    rt().block_on(async { mgr.on_mkdir("/data").await.unwrap() });

    assert!(store.ops().contains(&StoreOp::Put {
        key: "files/data/".into(),
        cond: Precondition::IfNoneMatchAny,
    }));
    assert_eq!(store.get_raw("files/data/").unwrap(), b"");
}

#[test]
fn on_mkdir_existing_marker_is_ok() {
    let store = store();
    store.insert_raw("files/data/", b"");
    let (mgr, _fs) = instance(&store, base_config());
    rt().block_on(async { mgr.on_mkdir("/data").await.unwrap() });
    assert_eq!(store.keys_under("files/data/").len(), 1);
}

#[test]
fn on_rmdir_deletes_marker_and_missing_marker_is_ok() {
    let store = store();
    let (mgr, _fs) = instance(&store, base_config());
    rt().block_on(async {
        mgr.on_mkdir("/data").await.unwrap();
        mgr.on_rmdir("/data").await.unwrap();
        mgr.on_rmdir("/data").await.unwrap();
    });
    assert!(store.get_raw("files/data/").is_none());
}

#[test]
fn populate_creates_dirs_from_markers_without_get() {
    let store = store();
    store.insert_raw("files/empty/", b"");
    store.insert_raw("files/full/f.txt", b"x");
    let fs = MemFs::new();

    let cache = rt().block_on(async { populate_from_s3(store.as_ref(), &fs).await.unwrap() });

    assert!(fs.has_dir("/empty"));
    assert!(fs.has_dir("/full"));
    assert_eq!(fs.read_local("/full/f.txt").unwrap(), b"x");
    assert!(cache.has_dir("/empty"));
    assert!(cache.get("/full/f.txt").is_some());
    assert!(!store.ops().contains(&StoreOp::Get {
        key: "files/empty/".into()
    }));
}

#[test]
fn poll_creates_dir_from_marker_and_removes_when_marker_gone() {
    let store = store();
    let ((a, _), (b, fs_b)) = two_instances(&store, base_config());

    rt().block_on(async {
        a.on_mkdir("/shared").await.unwrap();
        b.maybe_sync().await;
    });
    assert!(fs_b.has_dir("/shared"));

    rt().block_on(async {
        a.on_rmdir("/shared").await.unwrap();
        b.maybe_sync().await;
    });
    assert!(!fs_b.has_dir("/shared"));
}

#[test]
fn list_files_maps_marker_keys_to_is_dir() {
    use vfs_sync_core::FileStore;
    let store = store();
    store.insert_raw("files/d/", b"");
    store.insert_raw("files/d/f", b"x");
    let listed = rt().block_on(async { store.list_files().await.unwrap() });
    let mut paths: Vec<(String, bool)> = listed.into_iter().map(|o| (o.path, o.is_dir)).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![("/d".to_string(), true), ("/d/f".to_string(), false)]
    );
}
