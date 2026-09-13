//! Object-store abstraction and the key layout used on top of it.
//!
//! `SyncManager` talks to S3 only through [`ObjectStore`]. The production
//! implementation is [`crate::S3Storage`]; tests use
//! [`crate::testing::MemoryObjectStore`], which reproduces S3's
//! compare-and-swap semantics for conditional writes in memory.
//!
//! Keys are relative to the sync prefix and laid out as
//!
//! ```text
//! files/<path>     file content
//! files/<dir>/     directory marker (empty object)
//! locks/<path>     lease record for <path>
//! ```

use crate::types::{ObjectMeta, Precondition, S3Error, S3ObjectInfo};

/// Namespace for file content and directory markers.
pub const FILES_PREFIX: &str = "files/";
/// Namespace for lease records.
pub const LOCKS_PREFIX: &str = "locks/";

/// Minimal object-store interface.
///
/// Futures deliberately carry no `Send` bound: the WASI consumers drive the
/// manager on a single thread over `Rc<RefCell<Fs>>`, and the native host
/// runs each call on a dedicated thread with its own runtime.
#[allow(async_fn_in_trait)]
pub trait ObjectStore {
    /// List objects whose key starts with `prefix`, up to `max_keys` when
    /// given. Keys come back relative to the sync prefix.
    async fn list(&self, prefix: &str, max_keys: Option<usize>)
        -> Result<Vec<ObjectMeta>, S3Error>;

    /// Metadata only. `Ok(None)` when the key does not exist.
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>, S3Error>;

    /// Content plus metadata. `Ok(None)` when the key does not exist.
    async fn get(&self, key: &str) -> Result<Option<(Vec<u8>, ObjectMeta)>, S3Error>;

    /// Write `body` and return the new ETag. The precondition is evaluated
    /// atomically by the store; a lost race surfaces as
    /// [`S3Error::PreconditionFailed`] (or [`S3Error::NotFound`] when an
    /// `IfMatch` target was deleted concurrently).
    async fn put(&self, key: &str, body: Vec<u8>, cond: Precondition) -> Result<String, S3Error>;

    /// Delete `key`. `IfMatch` makes the delete conditional on the current
    /// ETag. Deleting a missing key is not an error.
    async fn delete(&self, key: &str, cond: Precondition) -> Result<(), S3Error>;
}

/// `files/<path>` for a VFS path such as `/a/b`.
pub fn file_key(path: &str) -> String {
    format!("{}{}", FILES_PREFIX, path.trim_start_matches('/'))
}

/// `files/<dir>/` for a VFS directory path such as `/a`.
pub fn dir_marker_key(path: &str) -> String {
    format!("{}/", file_key(path.trim_end_matches('/')))
}

/// `locks/<path>` for a VFS path such as `/a/b`.
pub fn lock_key(path: &str) -> String {
    format!("{}{}", LOCKS_PREFIX, path.trim_start_matches('/'))
}

/// Map a `files/...` key back to `(vfs_path, is_dir)`. Returns `None` for
/// keys outside the namespace and for the bare `files/` prefix itself.
pub fn path_from_file_key(key: &str) -> Option<(String, bool)> {
    let rest = key.strip_prefix(FILES_PREFIX)?;
    if rest.is_empty() {
        return None;
    }
    let is_dir = rest.ends_with('/');
    let trimmed = rest.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some((format!("/{}", trimmed), is_dir))
}

/// Path-level helpers layered over [`ObjectStore`]. Blanket-implemented so
/// `SyncManager` can stay generic over the raw store.
#[allow(async_fn_in_trait)]
pub trait FileStore: ObjectStore {
    /// Every object under `files/`, expressed as VFS paths. Directory
    /// markers come back with `is_dir == true`.
    async fn list_files(&self) -> Result<Vec<S3ObjectInfo>, S3Error> {
        let metas = self.list(FILES_PREFIX, None).await?;
        Ok(metas
            .into_iter()
            .filter_map(|m| {
                let (path, is_dir) = path_from_file_key(&m.key)?;
                Some(S3ObjectInfo {
                    path,
                    etag: m.etag,
                    last_modified: m.last_modified,
                    size: m.size,
                    is_dir,
                })
            })
            .collect())
    }

    /// `(etag, last_modified, size)` for `files/<path>`.
    async fn head_file(&self, path: &str) -> Result<Option<(String, u64, u64)>, S3Error> {
        Ok(self
            .head(&file_key(path))
            .await?
            .map(|m| (m.etag, m.last_modified, m.size)))
    }

    /// `(content, etag, last_modified)` for `files/<path>`.
    async fn get_file(&self, path: &str) -> Result<Option<(Vec<u8>, String, u64)>, S3Error> {
        Ok(self
            .get(&file_key(path))
            .await?
            .map(|(body, m)| (body, m.etag, m.last_modified)))
    }

    async fn put_file(
        &self,
        path: &str,
        data: Vec<u8>,
        cond: Precondition,
    ) -> Result<String, S3Error> {
        self.put(&file_key(path), data, cond).await
    }

    async fn delete_file(&self, path: &str, cond: Precondition) -> Result<(), S3Error> {
        self.delete(&file_key(path), cond).await
    }

    /// ETag of the directory marker for `path`, if one exists.
    async fn head_dir_marker(&self, path: &str) -> Result<Option<String>, S3Error> {
        Ok(self.head(&dir_marker_key(path)).await?.map(|m| m.etag))
    }

    /// Create the directory marker. Returns `Some(etag)` when this call
    /// created it and `None` when it already existed; another instance
    /// creating the same directory is not a conflict.
    async fn put_dir_marker(&self, path: &str) -> Result<Option<String>, S3Error> {
        match self
            .put(
                &dir_marker_key(path),
                Vec::new(),
                Precondition::IfNoneMatchAny,
            )
            .await
        {
            Ok(etag) => Ok(Some(etag)),
            Err(e) if e.is_precondition_failed() => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn delete_dir_marker(&self, path: &str, cond: Precondition) -> Result<(), S3Error> {
        self.delete(&dir_marker_key(path), cond).await
    }

    /// Whether any object exists under `files/<path>/`.
    async fn has_children(&self, path: &str) -> Result<bool, S3Error> {
        let under = self.list(&dir_marker_key(path), Some(2)).await?;
        Ok(under.iter().any(|m| m.key != dir_marker_key(path)))
    }
}

impl<S: ObjectStore> FileStore for S {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_key_normalises_leading_slash() {
        assert_eq!(file_key("/a/b"), "files/a/b");
        assert_eq!(file_key("a/b"), "files/a/b");
        assert_eq!(file_key("/logs/app.log"), "files/logs/app.log");
    }

    #[test]
    fn dir_marker_key_has_trailing_slash() {
        assert_eq!(dir_marker_key("/a"), "files/a/");
        assert_eq!(dir_marker_key("/a/"), "files/a/");
    }

    #[test]
    fn lock_key_lives_under_locks_prefix() {
        assert_eq!(lock_key("/logs/app.log"), "locks/logs/app.log");
    }

    #[test]
    fn path_from_file_key_detects_markers() {
        assert_eq!(
            path_from_file_key("files/a/b"),
            Some(("/a/b".to_string(), false))
        );
        assert_eq!(
            path_from_file_key("files/a/b/"),
            Some(("/a/b".to_string(), true))
        );
    }

    #[test]
    fn path_from_file_key_skips_bare_prefix_and_foreign_keys() {
        assert_eq!(path_from_file_key("files/"), None);
        assert_eq!(path_from_file_key("locks/a"), None);
        assert_eq!(path_from_file_key("other"), None);
    }
}
