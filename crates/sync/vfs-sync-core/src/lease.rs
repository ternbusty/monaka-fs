//! Per-file leases stored as S3 objects.
//!
//! A lease is a lock with an expiry. It is acquired by creating
//! `locks/<path>` with `If-None-Match: *`, which S3 evaluates atomically so
//! at most one instance wins. An expired lease is taken over with
//! `If-Match` on the stale record's ETag, again atomic. Release deletes the
//! record with `If-Match` on our own ETag so we never remove a lease that
//! was taken over from us.
//!
//! Expiry is wall-clock based (Unix milliseconds) because it must be
//! comparable across instances. Clock skew only shifts when a takeover
//! becomes possible; data safety does not depend on it because every data
//! write is fenced with `If-Match` on the data object's ETag.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::time::Instant;

use crate::object_store::{lock_key, ObjectStore};
use crate::types::{Precondition, S3Error, SyncError};

/// Wire format of a lease record (plain `key=value` lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRecord {
    pub instance: String,
    pub epoch: u64,
    /// Unix milliseconds after which the lease may be taken over.
    pub expires_ms: u64,
    pub path: String,
}

impl LockRecord {
    pub fn encode(&self) -> Vec<u8> {
        format!(
            "instance={}\nepoch={}\nexpires={}\npath={}\n",
            self.instance, self.epoch, self.expires_ms, self.path
        )
        .into_bytes()
    }

    /// `None` for unparsable bodies; callers treat that as expired so a
    /// corrupt record never wedges a path.
    pub fn parse(body: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(body).ok()?;
        let mut instance = None;
        let mut epoch = None;
        let mut expires = None;
        let mut path = None;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "instance" => instance = Some(v.trim().to_string()),
                "epoch" => epoch = v.trim().parse().ok(),
                "expires" => expires = v.trim().parse().ok(),
                "path" => path = Some(v.trim().to_string()),
                _ => {}
            }
        }
        Some(Self {
            instance: instance?,
            epoch: epoch?,
            expires_ms: expires?,
            path: path.unwrap_or_default(),
        })
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_ms
    }
}

/// Local bookkeeping for a lease this instance holds.
#[derive(Debug, Clone)]
pub struct WriteLease {
    /// ETag of `files/<path>` when the lease was taken (or after the last
    /// PUT). `None` means the object did not exist, so the next write uses
    /// `If-None-Match: *`.
    pub base_etag: Option<String>,
    /// ETag of our `locks/<path>` record, for renew / release CAS.
    pub lock_etag: String,
    pub epoch: u64,
    /// Local monotonic mirror of the record's expiry, for renewal timing.
    pub expires_at: Instant,
    /// Number of local write opens sharing this lease.
    pub refcount: usize,
    /// Written locally since the last PUT.
    pub dirty: bool,
    /// Unlinked locally while leased; close performs the delete.
    pub pending_delete: bool,
    /// A renewal CAS failed: someone took the lease over. Close still
    /// attempts the fenced PUT, which fails if they wrote.
    pub lost: bool,
    /// The last closer is flushing and releasing; new opens wait.
    pub releasing: bool,
}

#[derive(Default)]
pub struct LeaseTable {
    map: HashMap<String, WriteLease>,
}

impl LeaseTable {
    pub fn get(&self, path: &str) -> Option<&WriteLease> {
        self.map.get(path)
    }

    pub fn get_mut(&mut self, path: &str) -> Option<&mut WriteLease> {
        self.map.get_mut(path)
    }

    pub fn insert(&mut self, path: &str, lease: WriteLease) {
        self.map.insert(path.to_string(), lease);
    }

    pub fn remove(&mut self, path: &str) -> Option<WriteLease> {
        self.map.remove(path)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.map.contains_key(path)
    }

    pub fn paths(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// Paths whose lease is past half of its lifetime and should be renewed.
    pub fn paths_needing_renewal(&self, now: Instant, lease: Duration) -> Vec<String> {
        let half = lease / 2;
        self.map
            .iter()
            .filter(|(_, l)| !l.releasing && !l.lost)
            .filter(|(_, l)| l.expires_at.saturating_duration_since(now) <= half)
            .map(|(p, _)| p.clone())
            .collect()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Random-looking instance id without pulling in a `rand` dependency.
/// `RandomState` is seeded from the OS (via `wasi:random` on WASI).
pub fn generate_instance_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut h1 = std::hash::RandomState::new().build_hasher();
    h1.write_u128(nanos);
    let a = h1.finish();
    let mut h2 = std::hash::RandomState::new().build_hasher();
    h2.write_u64(a);
    let b = h2.finish();
    format!("{:016x}{:016x}", a, b)
}

fn jitter(step: u64) -> u64 {
    let mut h = std::hash::RandomState::new().build_hasher();
    h.write_u64(step);
    h.finish() % 20
}

/// Outcome of a successful acquisition.
#[derive(Debug, Clone)]
pub struct Acquired {
    pub lock_etag: String,
    pub epoch: u64,
    pub expires_at: Instant,
}

/// GET the lock and check whether our PUT actually landed despite a
/// reported error (transport failure, spurious 412). Returns the lock
/// ETag when the stored record matches our `instance` and `epoch`.
async fn verify_lease_landed<S: ObjectStore>(
    store: &S,
    key: &str,
    instance: &str,
    epoch: u64,
) -> Option<String> {
    let (body, meta) = store.get(key).await.ok().flatten()?;
    let record = LockRecord::parse(&body)?;
    if record.instance == instance && record.epoch == epoch {
        Some(meta.etag)
    } else {
        None
    }
}

/// Acquire the lease for `path`, waiting up to `timeout` for a live holder
/// to release it. Expired records are taken over.
pub async fn acquire<S: ObjectStore>(
    store: &S,
    path: &str,
    instance: &str,
    lease: Duration,
    timeout: Duration,
) -> Result<Acquired, SyncError> {
    let key = lock_key(path);
    let deadline = Instant::now() + timeout;
    let mut backoff = Duration::from_millis(50);
    let mut step: u64 = 0;
    let mut holder: Option<String> = None;

    loop {
        let attempt: Result<(), S3Error> = match store.get(&key).await? {
            None => {
                let record = LockRecord {
                    instance: instance.to_string(),
                    epoch: 1,
                    expires_ms: now_ms() + lease.as_millis() as u64,
                    path: path.to_string(),
                };
                let expires_at = Instant::now() + lease;
                match store
                    .put(&key, record.encode(), Precondition::IfNoneMatchAny)
                    .await
                {
                    Ok(lock_etag) => {
                        return Ok(Acquired {
                            lock_etag,
                            epoch: 1,
                            expires_at,
                        })
                    }
                    Err(put_err) => match verify_lease_landed(store, &key, instance, 1).await {
                        Some(lock_etag) => {
                            log::warn!(
                                "[sync] Lease PUT for {} failed ({}) but GET confirms it landed",
                                path,
                                put_err
                            );
                            return Ok(Acquired {
                                lock_etag,
                                epoch: 1,
                                expires_at,
                            });
                        }
                        None if put_err.is_precondition_failed() => Err(put_err),
                        None => return Err(put_err.into()),
                    },
                }
            }
            Some((body, meta)) => {
                let record = LockRecord::parse(&body);
                let expired = record.as_ref().is_none_or(|r| r.is_expired(now_ms()));
                if expired {
                    let epoch = record.as_ref().map_or(1, |r| r.epoch + 1);
                    let fresh = LockRecord {
                        instance: instance.to_string(),
                        epoch,
                        expires_ms: now_ms() + lease.as_millis() as u64,
                        path: path.to_string(),
                    };
                    let expires_at = Instant::now() + lease;
                    let prev_holder = record.map(|r| r.instance).unwrap_or_default();
                    match store
                        .put(&key, fresh.encode(), Precondition::IfMatch(meta.etag))
                        .await
                    {
                        Ok(lock_etag) => {
                            log::warn!(
                                "[sync] Took over expired lease for {} from {}",
                                path,
                                prev_holder
                            );
                            return Ok(Acquired {
                                lock_etag,
                                epoch,
                                expires_at,
                            });
                        }
                        Err(put_err) => {
                            if let Some(lock_etag) =
                                verify_lease_landed(store, &key, instance, epoch).await
                            {
                                log::warn!(
                                    "[sync] Takeover PUT for {} failed ({}) but GET confirms it landed",
                                    path, put_err
                                );
                                return Ok(Acquired {
                                    lock_etag,
                                    epoch,
                                    expires_at,
                                });
                            }
                            if put_err.is_precondition_failed() || put_err.is_not_found() {
                                Err(put_err)
                            } else {
                                return Err(put_err.into());
                            }
                        }
                    }
                } else {
                    holder = record.map(|r| r.instance);
                    Err(S3Error::PreconditionFailed {
                        key: key.clone(),
                        status: 412,
                        code: "LeaseHeld".into(),
                    })
                }
            }
        };

        // `attempt` is always `Err` here; the `Ok` arms returned above.
        let _ = attempt;

        let now = Instant::now();
        if now >= deadline {
            return Err(SyncError::Busy {
                path: path.to_string(),
                holder,
            });
        }
        step += 1;
        let wait = backoff + Duration::from_millis(jitter(step));
        let remaining = deadline.saturating_duration_since(now);
        tokio::time::sleep(wait.min(remaining)).await;
        backoff = (backoff * 2).min(Duration::from_secs(1));
    }
}

/// Extend our lease. Returns the new lock ETag. A `PreconditionFailed` or
/// `NotFound` means the lease was taken over or released elsewhere.
pub async fn renew<S: ObjectStore>(
    store: &S,
    path: &str,
    instance: &str,
    epoch: u64,
    lock_etag: &str,
    lease: Duration,
) -> Result<(String, Instant), S3Error> {
    let record = LockRecord {
        instance: instance.to_string(),
        epoch,
        expires_ms: now_ms() + lease.as_millis() as u64,
        path: path.to_string(),
    };
    let expires_at = Instant::now() + lease;
    let etag = store
        .put(
            &lock_key(path),
            record.encode(),
            Precondition::IfMatch(lock_etag.to_string()),
        )
        .await?;
    Ok((etag, expires_at))
}

/// Delete our lease record. Failures are logged and swallowed: a lease we
/// no longer own must not be deleted, and one we still own expires anyway.
pub async fn release<S: ObjectStore>(store: &S, path: &str, lock_etag: &str) {
    match store
        .delete(
            &lock_key(path),
            Precondition::IfMatch(lock_etag.to_string()),
        )
        .await
    {
        Ok(()) => {}
        Err(e) if e.is_precondition_failed() || e.is_not_found() => {
            log::warn!(
                "[sync] Lease for {} was taken over before release; leaving it",
                path
            );
        }
        Err(e) => {
            log::error!("[sync] Failed to release lease for {}: {}", path, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_record_roundtrip() {
        let r = LockRecord {
            instance: "abc".into(),
            epoch: 3,
            expires_ms: 1_700_000_000_123,
            path: "/a/b".into(),
        };
        assert_eq!(LockRecord::parse(&r.encode()), Some(r));
    }

    #[test]
    fn lock_record_ignores_unknown_lines_and_requires_core_fields() {
        let ok = b"instance=x\nepoch=1\nexpires=5\nextra=ignored\n";
        assert!(LockRecord::parse(ok).is_some());
        assert!(LockRecord::parse(b"garbage").is_none());
        assert!(LockRecord::parse(b"instance=x\nepoch=1\n").is_none());
    }

    #[test]
    fn expiry_is_inclusive_of_deadline() {
        let r = LockRecord {
            instance: "x".into(),
            epoch: 1,
            expires_ms: 100,
            path: String::new(),
        };
        assert!(!r.is_expired(99));
        assert!(r.is_expired(100));
    }

    #[test]
    fn instance_ids_differ_between_calls() {
        assert_ne!(generate_instance_id(), generate_instance_id());
        assert_eq!(generate_instance_id().len(), 32);
    }
}
