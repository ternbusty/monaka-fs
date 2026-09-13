//! Test doubles shared by this crate's tests and by downstream crates.
//!
//! [`MemoryObjectStore`] reproduces the parts of S3 that the sync logic
//! depends on: keys, ETags derived from content, and atomic evaluation of
//! `If-Match` / `If-None-Match: *`. Several `SyncManager`s sharing one
//! store model several instances sharing one bucket.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use crate::object_store::ObjectStore;
use crate::types::{ObjectMeta, Precondition, S3Error};

/// One recorded store call, for assertions on what the manager sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOp {
    List { prefix: String },
    Head { key: String },
    Get { key: String },
    Put { key: String, cond: Precondition },
    Delete { key: String, cond: Precondition },
}

#[derive(Debug, Clone)]
struct MemObject {
    data: Vec<u8>,
    etag: String,
    last_modified: u64,
}

#[derive(Default)]
struct Inner {
    objects: BTreeMap<String, MemObject>,
    ops: Vec<StoreOp>,
    fail_next: VecDeque<S3Error>,
    /// Monotonic stand-in for S3's last-modified clock.
    clock: u64,
    /// When set, conditional deletes fail with `NotImplemented`, mimicking
    /// LocalStack 4.x.
    reject_conditional_delete: bool,
}

/// In-memory [`ObjectStore`] with compare-and-swap semantics.
#[derive(Default)]
pub struct MemoryObjectStore {
    inner: Mutex<Inner>,
}

impl MemoryObjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Content-derived ETag, so equal bodies get equal ETags like S3's MD5.
    pub fn etag_for(data: &[u8]) -> String {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut h);
        format!("{:016x}", h.finish())
    }

    /// Insert or replace an object without going through `put`, as if
    /// another client wrote it. Returns the new ETag.
    pub fn insert_raw(&self, key: &str, data: &[u8]) -> String {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let last_modified = inner.clock;
        let etag = Self::etag_for(data);
        inner.objects.insert(
            key.to_string(),
            MemObject {
                data: data.to_vec(),
                etag: etag.clone(),
                last_modified,
            },
        );
        etag
    }

    pub fn remove_raw(&self, key: &str) {
        self.inner.lock().unwrap().objects.remove(key);
    }

    pub fn get_raw(&self, key: &str) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .objects
            .get(key)
            .map(|o| o.data.clone())
    }

    pub fn etag_of(&self, key: &str) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .objects
            .get(key)
            .map(|o| o.etag.clone())
    }

    pub fn set_last_modified(&self, key: &str, value: u64) {
        if let Some(o) = self.inner.lock().unwrap().objects.get_mut(key) {
            o.last_modified = value;
        }
    }

    pub fn keys(&self) -> Vec<String> {
        self.inner.lock().unwrap().objects.keys().cloned().collect()
    }

    pub fn keys_under(&self, prefix: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .objects
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }

    pub fn ops(&self) -> Vec<StoreOp> {
        self.inner.lock().unwrap().ops.clone()
    }

    pub fn clear_ops(&self) {
        self.inner.lock().unwrap().ops.clear();
    }

    /// Make the next store call fail with `err` (queued, first in first out).
    pub fn fail_next(&self, err: S3Error) {
        self.inner.lock().unwrap().fail_next.push_back(err);
    }

    pub fn set_reject_conditional_delete(&self, reject: bool) {
        self.inner.lock().unwrap().reject_conditional_delete = reject;
    }

    fn take_failure(inner: &mut Inner) -> Result<(), S3Error> {
        match inner.fail_next.pop_front() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn check_cond(inner: &Inner, key: &str, cond: &Precondition) -> Result<(), S3Error> {
        let current = inner.objects.get(key);
        match cond {
            Precondition::None => Ok(()),
            Precondition::IfNoneMatchAny => {
                if current.is_some() {
                    Err(S3Error::PreconditionFailed {
                        key: key.to_string(),
                        status: 412,
                        code: "PreconditionFailed".into(),
                    })
                } else {
                    Ok(())
                }
            }
            Precondition::IfMatch(expected) => match current {
                None => Err(S3Error::NotFound {
                    key: key.to_string(),
                }),
                Some(o) if &o.etag == expected => Ok(()),
                Some(_) => Err(S3Error::PreconditionFailed {
                    key: key.to_string(),
                    status: 412,
                    code: "PreconditionFailed".into(),
                }),
            },
        }
    }
}

impl ObjectStore for MemoryObjectStore {
    async fn list(
        &self,
        prefix: &str,
        max_keys: Option<usize>,
    ) -> Result<Vec<ObjectMeta>, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.ops.push(StoreOp::List {
            prefix: prefix.to_string(),
        });
        Self::take_failure(&mut inner)?;
        let iter = inner
            .objects
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, o)| ObjectMeta {
                key: k.clone(),
                etag: o.etag.clone(),
                last_modified: o.last_modified,
                size: o.data.len() as u64,
            });
        Ok(match max_keys {
            Some(n) => iter.take(n).collect(),
            None => iter.collect(),
        })
    }

    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.ops.push(StoreOp::Head {
            key: key.to_string(),
        });
        Self::take_failure(&mut inner)?;
        Ok(inner.objects.get(key).map(|o| ObjectMeta {
            key: key.to_string(),
            etag: o.etag.clone(),
            last_modified: o.last_modified,
            size: o.data.len() as u64,
        }))
    }

    async fn get(&self, key: &str) -> Result<Option<(Vec<u8>, ObjectMeta)>, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.ops.push(StoreOp::Get {
            key: key.to_string(),
        });
        Self::take_failure(&mut inner)?;
        Ok(inner.objects.get(key).map(|o| {
            (
                o.data.clone(),
                ObjectMeta {
                    key: key.to_string(),
                    etag: o.etag.clone(),
                    last_modified: o.last_modified,
                    size: o.data.len() as u64,
                },
            )
        }))
    }

    async fn put(&self, key: &str, body: Vec<u8>, cond: Precondition) -> Result<String, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.ops.push(StoreOp::Put {
            key: key.to_string(),
            cond: cond.clone(),
        });
        Self::take_failure(&mut inner)?;
        Self::check_cond(&inner, key, &cond)?;
        inner.clock += 1;
        let etag = Self::etag_for(&body);
        let last_modified = inner.clock;
        inner.objects.insert(
            key.to_string(),
            MemObject {
                data: body,
                etag: etag.clone(),
                last_modified,
            },
        );
        Ok(etag)
    }

    async fn delete(&self, key: &str, cond: Precondition) -> Result<(), S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.ops.push(StoreOp::Delete {
            key: key.to_string(),
            cond: cond.clone(),
        });
        Self::take_failure(&mut inner)?;
        if inner.reject_conditional_delete && cond != Precondition::None {
            return Err(S3Error::NotImplemented {
                key: key.to_string(),
                message: "conditional delete rejected by test store".into(),
            });
        }
        match &cond {
            // S3 treats deleting a missing key as success.
            Precondition::IfMatch(_) if !inner.objects.contains_key(key) => return Ok(()),
            _ => Self::check_cond(&inner, key, &cond)?,
        }
        inner.objects.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn put_if_none_match_fails_when_present() {
        let s = MemoryObjectStore::new();
        rt().block_on(async {
            s.put("k", b"a".to_vec(), Precondition::IfNoneMatchAny)
                .await
                .unwrap();
            let err = s
                .put("k", b"b".to_vec(), Precondition::IfNoneMatchAny)
                .await
                .unwrap_err();
            assert!(err.is_precondition_failed());
            assert_eq!(s.get_raw("k").unwrap(), b"a");
        });
    }

    #[test]
    fn put_if_match_succeeds_on_current_and_fails_on_stale_etag() {
        let s = MemoryObjectStore::new();
        rt().block_on(async {
            let e1 = s.put("k", b"a".to_vec(), Precondition::None).await.unwrap();
            let e2 = s
                .put("k", b"b".to_vec(), Precondition::IfMatch(e1.clone()))
                .await
                .unwrap();
            assert_ne!(e1, e2);
            let err = s
                .put("k", b"c".to_vec(), Precondition::IfMatch(e1))
                .await
                .unwrap_err();
            assert!(err.is_precondition_failed());
            assert_eq!(s.get_raw("k").unwrap(), b"b");
        });
    }

    #[test]
    fn put_if_match_on_missing_key_is_not_found() {
        let s = MemoryObjectStore::new();
        rt().block_on(async {
            let err = s
                .put("k", b"a".to_vec(), Precondition::IfMatch("x".into()))
                .await
                .unwrap_err();
            assert!(err.is_not_found());
        });
    }

    #[test]
    fn delete_if_match_fails_on_stale_etag_and_keeps_object() {
        let s = MemoryObjectStore::new();
        rt().block_on(async {
            let e1 = s.put("k", b"a".to_vec(), Precondition::None).await.unwrap();
            s.put("k", b"b".to_vec(), Precondition::None).await.unwrap();
            let err = s.delete("k", Precondition::IfMatch(e1)).await.unwrap_err();
            assert!(err.is_precondition_failed());
            assert!(s.get_raw("k").is_some());
            let e2 = s.etag_of("k").unwrap();
            s.delete("k", Precondition::IfMatch(e2)).await.unwrap();
            assert!(s.get_raw("k").is_none());
        });
    }

    #[test]
    fn delete_missing_key_is_ok() {
        let s = MemoryObjectStore::new();
        rt().block_on(async {
            s.delete("k", Precondition::None).await.unwrap();
            s.delete("k", Precondition::IfMatch("x".into()))
                .await
                .unwrap();
        });
    }

    #[test]
    fn list_respects_prefix_and_max_keys() {
        let s = MemoryObjectStore::new();
        s.insert_raw("files/a", b"1");
        s.insert_raw("files/b", b"2");
        s.insert_raw("locks/a", b"3");
        rt().block_on(async {
            let all = s.list("files/", None).await.unwrap();
            assert_eq!(all.len(), 2);
            let one = s.list("files/", Some(1)).await.unwrap();
            assert_eq!(one.len(), 1);
            assert_eq!(one[0].key, "files/a");
        });
    }

    #[test]
    fn etag_is_content_hash() {
        let s = MemoryObjectStore::new();
        let a = s.insert_raw("x", b"same");
        let b = s.insert_raw("y", b"same");
        let c = s.insert_raw("z", b"other");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn fail_next_injects_error_once() {
        let s = MemoryObjectStore::new();
        s.fail_next(S3Error::Read {
            key: "k".into(),
            message: "boom".into(),
        });
        rt().block_on(async {
            assert!(s.head("k").await.is_err());
            assert!(s.head("k").await.is_ok());
        });
    }
}
