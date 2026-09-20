//! Bidirectional S3 sync manager (host- and WASI-shared).
//!
//! Generic over [`FsBackend`] so that both the multi-threaded native host
//! (`Arc<fs_core::Fs>`) and the single-threaded WASI adapter
//! (`Rc<RefCell<fs_core::Fs<T>>>`) can share the same logic, and over
//! [`ObjectStore`] so tests can substitute an in-memory S3.
//!
//! State is kept behind `std::sync::Mutex`, which is correct in both cases
//! (single-thread WASI never contends). Every method follows one rule:
//! take a guard, copy what is needed, drop the guard, then `.await`. A
//! guard held across an await would block the host's hook threads and
//! deadlock the WASI single thread.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::time::Instant;

use crate::config::{MetadataMode, SyncConfig, SyncMode, SyncOperation};
use crate::file_metadata::MetadataCache;
use crate::fs_backend::FsBackend;
use crate::lease::{self, LeaseTable, WriteLease};
use crate::object_store::{FileStore, ObjectStore};
use crate::s3_client::S3Storage;
use crate::types::{Precondition, S3Error, S3ObjectInfo, SyncError};

/// Statistics from inbound sync.
#[derive(Default)]
pub struct SyncStats {
    pub downloaded: usize,
    pub deleted: usize,
}

/// Errors when initialising the filesystem from S3.
#[derive(Debug)]
pub enum LoadError {
    S3 { source: S3Error },
    Fs { message: String },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::S3 { source } => write!(f, "S3 error: {}", source),
            LoadError::Fs { message } => write!(f, "Filesystem error: {}", message),
        }
    }
}

impl std::error::Error for LoadError {}

/// Manages bidirectional S3 synchronization.
///
/// The `Send + Sync` story is determined by the chosen `F` and `S`. With
/// `F = Arc<Fs>` the manager is `Send + Sync` and can be shared via
/// `Arc<SyncManager<…>>` across threads (host); with
/// `F = Rc<RefCell<Fs<T>>>` it is `!Send`, matching single-threaded WASI use.
pub struct SyncManager<F: FsBackend, S: ObjectStore = S3Storage> {
    store: Arc<S>,
    fs: F,
    outbound_queue: Mutex<VecDeque<SyncOperation>>,
    metadata_cache: Mutex<MetadataCache>,
    leases: Mutex<LeaseTable>,
    instance_id: String,
    config: SyncConfig,
    last_poll: Mutex<Instant>,
    last_flush: Mutex<Instant>,
    shutdown: AtomicBool,
}

fn base_cond(base: &Option<String>) -> Precondition {
    match base {
        Some(etag) => Precondition::IfMatch(etag.clone()),
        None => Precondition::IfNoneMatchAny,
    }
}

fn fs_error(path: &str, e: S3Error) -> SyncError {
    SyncError::Fs {
        path: path.to_string(),
        message: e.to_string(),
    }
}

impl<F: FsBackend, S: ObjectStore> SyncManager<F, S> {
    /// Create a new sync manager.
    pub fn new(store: Arc<S>, fs: F, metadata_cache: MetadataCache, config: SyncConfig) -> Self {
        Self {
            store,
            fs,
            outbound_queue: Mutex::new(VecDeque::new()),
            metadata_cache: Mutex::new(metadata_cache),
            leases: Mutex::new(LeaseTable::default()),
            instance_id: lease::generate_instance_id(),
            config,
            last_poll: Mutex::new(Instant::now()),
            last_flush: Mutex::new(Instant::now()),
            shutdown: AtomicBool::new(false),
        }
    }

    /// Random id identifying this instance in lease records.
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Whether per-file leases are enabled (`VFS_S3_FILE_LOCK`).
    pub fn file_lock_enabled(&self) -> bool {
        self.config.file_lock
    }

    /// Whether this instance currently holds the lease for `path`.
    pub fn is_locked(&self, path: &str) -> bool {
        self.leases.lock().unwrap().contains(path)
    }

    /// Enqueue a file upload (deduping any prior op for the same path).
    /// While `path` is leased the write is only marked dirty; the close of
    /// the last write descriptor pushes it under the lease.
    pub fn enqueue_upload(&self, path: String) {
        if let Some(l) = self.leases.lock().unwrap().get_mut(&path) {
            l.dirty = true;
            l.pending_delete = false;
            return;
        }

        let mut queue = self.outbound_queue.lock().unwrap();
        queue.retain(|op| op.path() != path);
        queue.push_back(SyncOperation::Upload { path });
    }

    /// Enqueue a file deletion (also drops any cached metadata, stashing
    /// the last known ETag on the operation so the delete is conditional).
    pub fn enqueue_delete(&self, path: String) {
        if let Some(l) = self.leases.lock().unwrap().get_mut(&path) {
            l.pending_delete = true;
            l.dirty = false;
            return;
        }

        let etag = {
            let mut cache = self.metadata_cache.lock().unwrap();
            let etag = cache.get(&path).map(|m| m.etag.clone());
            cache.remove(&path);
            etag
        };

        let mut queue = self.outbound_queue.lock().unwrap();
        queue.retain(|op| op.path() != path);
        queue.push_back(SyncOperation::Delete { path, etag });
    }

    /// Number of pending outbound operations.
    pub fn pending_count(&self) -> usize {
        self.outbound_queue.lock().unwrap().len()
    }

    /// Whether realtime sync mode is configured.
    pub fn is_realtime(&self) -> bool {
        self.config.mode == SyncMode::RealTime
    }

    /// Whether a shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Request shutdown. The next `maybe_sync` call becomes a no-op.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    // -----------------------------------------------------------------
    // Outbound
    // -----------------------------------------------------------------

    /// Precondition for the next write of `path`: the lease base if held,
    /// otherwise the cached ETag, otherwise "must not exist".
    fn cond_for_upload(&self, path: &str) -> Precondition {
        if let Some(l) = self.leases.lock().unwrap().get(path) {
            return base_cond(&l.base_etag);
        }
        match self.metadata_cache.lock().unwrap().get(path) {
            Some(m) => Precondition::IfMatch(m.etag.clone()),
            None => Precondition::IfNoneMatchAny,
        }
    }

    /// Synchronously upload a single file to S3 (used by realtime hooks).
    pub async fn sync_file_now(&self, path: &str) -> Result<(), SyncError> {
        let cond = self.cond_for_upload(path);
        match self.upload_with_cond(path, cond).await {
            Ok(_) => Ok(()),
            Err(SyncError::S3(e)) if e.is_precondition_failed() => {
                self.handle_upload_conflict(path).await
            }
            Err(e) => Err(e),
        }
    }

    /// Read `path` from the local FS and PUT it with `cond`, then record the
    /// new ETag in the cache (and the lease base if held).
    ///
    /// A 404 on `If-Match` means the object was deleted concurrently; the
    /// local write is newer intent than that delete, so it is retried as a
    /// create. A 412 propagates for the caller to resolve.
    ///
    /// A transport error (timeout, connection reset) does not guarantee
    /// the PUT failed: S3 may have committed the object before the
    /// response was lost. When a conditional PUT gets a non-precondition
    /// error, HEAD the object to check whether the write actually landed.
    async fn upload_with_cond(&self, path: &str, cond: Precondition) -> Result<String, SyncError> {
        let content = self
            .read_file_content(path)
            .map_err(|e| fs_error(path, e))?;
        let size = content.len() as u64;
        let local_modified = self.fs.stat_modified(path);

        // S3 passthrough mode: pre-write existence checks (matches s3fs-fuse).
        if self.config.metadata_mode == MetadataMode::S3 {
            let (file_check, dir_check, children_check) = tokio::join!(
                self.store.head_file(path),
                self.store.head_dir_marker(path),
                self.store.has_children(path),
            );
            file_check?;
            dir_check?;
            children_check?;
        }

        let retry_as_create = matches!(cond, Precondition::IfMatch(_));
        let retry_as_overwrite = matches!(cond, Precondition::IfNoneMatchAny);
        let cond_clone = cond.clone();
        let etag = match self.store.put_file(path, content.clone(), cond).await {
            Ok(etag) => etag,
            Err(e) if e.is_not_found() && retry_as_create => {
                log::warn!(
                    "[sync] {} was deleted in S3 while a local write was pending; recreating",
                    path
                );
                self.store
                    .put_file(path, content, Precondition::IfNoneMatchAny)
                    .await?
            }
            Err(e) if e.is_precondition_failed() && retry_as_overwrite => {
                match self.store.head_file(path).await? {
                    Some(_) => return Err(e.into()),
                    None => {
                        log::warn!(
                            "[sync] {} got spurious 412 on create (HEAD confirms no object); retrying",
                            path
                        );
                        self.store
                            .put_file(path, content, Precondition::IfNoneMatchAny)
                            .await?
                    }
                }
            }
            Err(e) if !e.is_precondition_failed() => {
                match self.verify_upload_landed(path, &cond_clone).await {
                    Some(etag) => etag,
                    None => return Err(e.into()),
                }
            }
            Err(e) => return Err(e.into()),
        };

        self.metadata_cache.lock().unwrap().update_after_upload(
            path,
            etag.clone(),
            size,
            local_modified,
        );
        if let Some(l) = self.leases.lock().unwrap().get_mut(path) {
            l.base_etag = Some(etag.clone());
            l.dirty = false;
        }

        log::info!("[sync] Uploaded: {}", path);
        Ok(etag)
    }

    /// After a non-precondition PUT error, HEAD the object to check whether
    /// the write actually committed. Returns `Some(etag)` when the object
    /// state is consistent with a successful write, `None` when the PUT
    /// genuinely failed.
    async fn verify_upload_landed(&self, path: &str, cond: &Precondition) -> Option<String> {
        let (remote_etag, _, _) = match self.store.head_file(path).await {
            Ok(Some(meta)) => meta,
            _ => return None,
        };

        let landed = match cond {
            // File must not have existed before our PUT. If it exists now,
            // our PUT is the only thing that could have created it.
            Precondition::IfNoneMatchAny => true,
            // We hold the lease, so only our PUT could have changed the
            // etag from the base value.
            Precondition::IfMatch(base) => remote_etag != *base,
            Precondition::None => false,
        };

        if landed {
            log::warn!(
                "[sync] PUT for {} reported a transport error but HEAD confirms the write landed",
                path
            );
            Some(remote_etag)
        } else {
            None
        }
    }

    /// A conditional upload of `path` was rejected. With file locks
    /// disabled the caller opted into last-writer-wins, so retry
    /// unconditionally. Otherwise the remote version wins: pull it into the
    /// local FS and report the conflict.
    async fn handle_upload_conflict(&self, path: &str) -> Result<(), SyncError> {
        if !self.config.file_lock {
            log::warn!(
                "[sync] {} changed in S3 since last sync; overwriting (file lock disabled)",
                path
            );
            self.upload_with_cond(path, Precondition::None).await?;
            return Ok(());
        }

        log::warn!(
            "[sync] {} changed in S3 concurrently; keeping the S3 version locally",
            path
        );
        let refreshed = self.refresh_local_from_remote(path).await;
        Err(SyncError::Conflict {
            path: path.to_string(),
            refreshed,
        })
    }

    /// Replace the local copy of `path` with the current S3 object, updating
    /// the cache and any held lease base. Returns `false` when the object
    /// no longer exists remotely (the local copy is left as is).
    async fn refresh_local_from_remote(&self, path: &str) -> bool {
        match self.store.get_file(path).await {
            Ok(Some((content, etag, last_modified))) => {
                if let Err(e) = self.write_file_content(path, &content) {
                    log::error!("[sync] Failed to refresh {} locally: {}", path, e);
                    return false;
                }
                self.metadata_cache.lock().unwrap().update_after_download(
                    path,
                    etag.clone(),
                    last_modified,
                    content.len() as u64,
                );
                if let Some(l) = self.leases.lock().unwrap().get_mut(path) {
                    l.base_etag = Some(etag);
                    l.dirty = false;
                }
                true
            }
            Ok(None) => {
                self.metadata_cache.lock().unwrap().remove(path);
                if let Some(l) = self.leases.lock().unwrap().get_mut(path) {
                    l.base_etag = None;
                }
                false
            }
            Err(e) => {
                log::error!("[sync] Failed to fetch {} after conflict: {}", path, e);
                false
            }
        }
    }

    // -----------------------------------------------------------------
    // Inbound (read-through / metadata mode)
    // -----------------------------------------------------------------

    /// Refresh a single file from S3 (read-through mode). No-op while this
    /// instance holds the lease: the local copy is authoritative then.
    pub async fn refresh_file_from_s3(&self, path: &str) -> Result<(), SyncError> {
        if self.is_locked(path) {
            return Ok(());
        }
        if let Some((content, etag, last_modified)) = self.store.get_file(path).await? {
            self.write_file_content(path, &content)
                .map_err(|e| fs_error(path, e))?;
            self.metadata_cache.lock().unwrap().update_after_download(
                path,
                etag,
                last_modified,
                content.len() as u64,
            );
            log::debug!("[sync] Refreshed from S3: {}", path);
        }
        Ok(())
    }

    /// HEAD then GET only if the ETag has changed (metadata sync mode).
    /// Returns `true` if the file was refreshed. No-op while leased.
    pub async fn check_and_refresh_from_s3(&self, path: &str) -> Result<bool, SyncError> {
        if self.is_locked(path) {
            return Ok(false);
        }
        let (_, refreshed) = self.head_and_refresh(path).await?;
        Ok(refreshed)
    }

    /// HEAD `path`; GET it into the local FS when the ETag differs from the
    /// cache. Returns `(remote_etag, refreshed)`; `remote_etag` is `None`
    /// when the object does not exist.
    async fn head_and_refresh(&self, path: &str) -> Result<(Option<String>, bool), SyncError> {
        let Some((s3_etag, _, _)) = self.store.head_file(path).await? else {
            log::debug!("[sync] File not found in S3: {}", path);
            return Ok((None, false));
        };

        let needs_refresh = {
            let cache = self.metadata_cache.lock().unwrap();
            match cache.get(path) {
                Some(local_meta) => s3_etag != local_meta.etag,
                None => true,
            }
        };

        if !needs_refresh {
            log::debug!("[sync] S3 metadata unchanged for: {}", path);
            return Ok((Some(s3_etag), false));
        }

        match self.store.get_file(path).await? {
            Some((content, etag, last_modified)) => {
                self.write_file_content(path, &content)
                    .map_err(|e| fs_error(path, e))?;
                self.metadata_cache.lock().unwrap().update_after_download(
                    path,
                    etag.clone(),
                    last_modified,
                    content.len() as u64,
                );
                log::debug!("[sync] Refreshed from S3 (metadata changed): {}", path);
                Ok((Some(etag), true))
            }
            // Deleted between HEAD and GET.
            None => Ok((None, false)),
        }
    }

    // -----------------------------------------------------------------
    // Leases
    // -----------------------------------------------------------------

    /// Acquire the per-file lease for `path` before it is opened for write,
    /// refresh the local copy from S3, and pin the data ETag. Re-entrant
    /// within one instance (reference counted). No-op when file locks are
    /// disabled.
    pub async fn on_open_write(&self, path: &str) -> Result<(), SyncError> {
        if !self.config.file_lock {
            return Ok(());
        }

        // Fast path: we already hold it. If the last closer is mid-release,
        // wait for it to finish rather than racing the lock object.
        loop {
            let releasing = {
                let mut leases = self.leases.lock().unwrap();
                match leases.get_mut(path) {
                    Some(l) if l.releasing => true,
                    Some(l) => {
                        l.refcount += 1;
                        return Ok(());
                    }
                    None => break,
                }
            };
            if releasing {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }

        let acquired = lease::acquire(
            self.store.as_ref(),
            path,
            &self.instance_id,
            self.config.lock_lease,
            self.config.lock_timeout,
        )
        .await?;

        // Another local thread may have inserted an entry while we were
        // acquiring (host only). Join it and give back our lock object.
        let already_held = {
            let mut leases = self.leases.lock().unwrap();
            match leases.get_mut(path) {
                Some(l) => {
                    l.refcount += 1;
                    true
                }
                None => false,
            }
        };
        if already_held {
            lease::release(self.store.as_ref(), path, &acquired.lock_etag).await;
            return Ok(());
        }

        // A queued (batch) upload for this path must land before we adopt
        // the remote version as base, or the refresh would clobber it.
        let had_pending = {
            let mut queue = self.outbound_queue.lock().unwrap();
            let before = queue.len();
            queue.retain(|op| !matches!(op, SyncOperation::Upload { path: p } if p == path));
            queue.len() != before
        };

        let base = if had_pending {
            let cond = self.cond_for_upload(path);
            match self.upload_with_cond(path, cond).await {
                Ok(etag) => Some(etag),
                Err(SyncError::S3(e)) if e.is_precondition_failed() => {
                    let refreshed = self.refresh_local_from_remote(path).await;
                    lease::release(self.store.as_ref(), path, &acquired.lock_etag).await;
                    return Err(SyncError::Conflict {
                        path: path.to_string(),
                        refreshed,
                    });
                }
                Err(e) => {
                    lease::release(self.store.as_ref(), path, &acquired.lock_etag).await;
                    return Err(e);
                }
            }
        } else {
            match self.head_and_refresh(path).await {
                Ok((base, _)) => base,
                Err(e) => {
                    lease::release(self.store.as_ref(), path, &acquired.lock_etag).await;
                    return Err(e);
                }
            }
        };

        self.leases.lock().unwrap().insert(
            path,
            WriteLease {
                base_etag: base,
                lock_etag: acquired.lock_etag,
                epoch: acquired.epoch,
                expires_at: acquired.expires_at,
                refcount: 1,
                dirty: false,
                pending_delete: false,
                lost: false,
                releasing: false,
            },
        );
        log::debug!("[sync] Lease acquired: {}", path);
        Ok(())
    }

    /// Release one reference to the lease for `path`. The last release
    /// pushes pending changes with `If-Match` on the pinned base and then
    /// deletes the lock object. No-op when `path` is not leased.
    pub async fn on_close(&self, path: &str) -> Result<(), SyncError> {
        let snapshot = {
            let mut leases = self.leases.lock().unwrap();
            match leases.get_mut(path) {
                None => return Ok(()),
                Some(l) if l.refcount > 1 => {
                    l.refcount -= 1;
                    return Ok(());
                }
                Some(l) => {
                    l.releasing = true;
                    l.clone()
                }
            }
        };

        let result = self.flush_lease(path, &snapshot).await;

        lease::release(self.store.as_ref(), path, &snapshot.lock_etag).await;
        self.leases.lock().unwrap().remove(path);
        log::debug!("[sync] Lease released: {}", path);
        result
    }

    /// Push the leased path's pending change (delete or upload).
    async fn flush_lease(&self, path: &str, snapshot: &WriteLease) -> Result<(), SyncError> {
        if snapshot.pending_delete {
            let Some(base) = &snapshot.base_etag else {
                // Never existed remotely; nothing to delete.
                self.metadata_cache.lock().unwrap().remove(path);
                return Ok(());
            };
            return match self
                .store
                .delete_file(path, Precondition::IfMatch(base.clone()))
                .await
            {
                Ok(()) => {
                    self.metadata_cache.lock().unwrap().remove(path);
                    log::info!("[sync] Deleted from S3: {}", path);
                    Ok(())
                }
                Err(e) if e.is_precondition_failed() => {
                    log::warn!(
                        "[sync] {} changed in S3 before local delete; restoring the S3 version",
                        path
                    );
                    let refreshed = self.refresh_local_from_remote(path).await;
                    Err(SyncError::Conflict {
                        path: path.to_string(),
                        refreshed,
                    })
                }
                Err(e) => Err(e.into()),
            };
        }

        if !snapshot.dirty {
            return Ok(());
        }

        match self
            .upload_with_cond(path, base_cond(&snapshot.base_etag))
            .await
        {
            Ok(_) => Ok(()),
            Err(SyncError::S3(e)) if e.is_precondition_failed() => {
                log::warn!(
                    "[sync] {} changed in S3 while leased (fencing); keeping the S3 version",
                    path
                );
                let refreshed = self.refresh_local_from_remote(path).await;
                Err(SyncError::Conflict {
                    path: path.to_string(),
                    refreshed,
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Push `path` now without releasing the lease (`fsync`). Without a
    /// lease this behaves like [`Self::sync_file_now`].
    pub async fn on_fsync(&self, path: &str) -> Result<(), SyncError> {
        let snapshot = self.leases.lock().unwrap().get(path).cloned();
        match snapshot {
            Some(l) if l.dirty || l.pending_delete => self.flush_lease(path, &l).await,
            Some(_) => Ok(()),
            None => self.sync_file_now(path).await,
        }
    }

    /// Create the directory marker for `path` in S3.
    pub async fn on_mkdir(&self, path: &str) -> Result<(), SyncError> {
        let etag = match self.store.put_dir_marker(path).await? {
            Some(etag) => Some(etag),
            None => self.store.head_dir_marker(path).await?,
        };
        if let Some(etag) = etag {
            self.metadata_cache.lock().unwrap().add_dir(path, etag);
        }
        log::debug!("[sync] Directory marker created: {}", path);
        Ok(())
    }

    /// Remove the directory marker for `path` from S3.
    pub async fn on_rmdir(&self, path: &str) -> Result<(), SyncError> {
        let cond = match self.metadata_cache.lock().unwrap().dir_etag(path) {
            Some(etag) => Precondition::IfMatch(etag.clone()),
            None => Precondition::None,
        };
        match self.store.delete_dir_marker(path, cond).await {
            Ok(()) => {}
            Err(e) if e.is_precondition_failed() => {
                log::warn!("[sync] Directory marker for {} changed; leaving it", path);
            }
            Err(e) => return Err(e.into()),
        }
        self.metadata_cache.lock().unwrap().remove_dir(path);
        log::debug!("[sync] Directory marker removed: {}", path);
        Ok(())
    }

    /// Flush every leased path and delete every lock object this instance
    /// holds. For shutdown. Returns the first error after processing all.
    pub async fn release_all_leases(&self) -> Result<(), SyncError> {
        let paths = self.leases.lock().unwrap().paths();
        let mut first_err = None;
        for path in paths {
            let snapshot = {
                let mut leases = self.leases.lock().unwrap();
                match leases.get_mut(&path) {
                    Some(l) => {
                        l.releasing = true;
                        l.clone()
                    }
                    None => continue,
                }
            };
            if let Err(e) = self.flush_lease(&path, &snapshot).await {
                log::error!("[sync] Failed to flush {} on shutdown: {}", path, e);
                first_err.get_or_insert(e);
            }
            lease::release(self.store.as_ref(), &path, &snapshot.lock_etag).await;
            self.leases.lock().unwrap().remove(&path);
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Extend leases that are past half their lifetime.
    async fn renew_leases(&self) {
        let due = self
            .leases
            .lock()
            .unwrap()
            .paths_needing_renewal(Instant::now(), self.config.lock_lease);
        for path in due {
            let Some((epoch, lock_etag)) = self
                .leases
                .lock()
                .unwrap()
                .get(&path)
                .map(|l| (l.epoch, l.lock_etag.clone()))
            else {
                continue;
            };
            match lease::renew(
                self.store.as_ref(),
                &path,
                &self.instance_id,
                epoch,
                &lock_etag,
                self.config.lock_lease,
            )
            .await
            {
                Ok((new_etag, expires_at)) => {
                    if let Some(l) = self.leases.lock().unwrap().get_mut(&path) {
                        l.lock_etag = new_etag;
                        l.expires_at = expires_at;
                    }
                }
                Err(e) if e.is_precondition_failed() || e.is_not_found() => {
                    log::warn!(
                        "[sync] Lease for {} was taken over by another instance",
                        path
                    );
                    if let Some(l) = self.leases.lock().unwrap().get_mut(&path) {
                        l.lost = true;
                    }
                }
                Err(e) => log::error!("[sync] Failed to renew lease for {}: {}", path, e),
            }
        }
    }

    // -----------------------------------------------------------------
    // Background tick
    // -----------------------------------------------------------------

    /// Cooperative sync check - call from background thread / event loop.
    pub async fn maybe_sync(&self) -> bool {
        if self.is_shutdown() {
            return false;
        }

        let mut did_work = false;

        if self.config.file_lock {
            self.renew_leases().await;
        }

        // In RealTime mode, uploads are handled inline by `sync_file_now` hooks,
        // so the background flush only handles pending Delete ops.
        let should_flush = {
            let queue = self.outbound_queue.lock().unwrap();
            let elapsed = self.last_flush.lock().unwrap().elapsed();

            match self.config.mode {
                SyncMode::RealTime => queue
                    .iter()
                    .any(|op| matches!(op, SyncOperation::Delete { .. })),
                SyncMode::Batch => {
                    queue.len() >= self.config.outbound_batch_size
                        || (!queue.is_empty() && elapsed >= self.config.flush_interval)
                }
            }
        };

        if should_flush {
            match self.flush_outbound().await {
                Ok(count) => {
                    if count > 0 {
                        log::debug!("[sync] Flushed {} operations to S3", count);
                        did_work = true;
                    }
                }
                Err(e) => {
                    log::error!("[sync] Outbound flush error: {}", e);
                }
            }
        }

        let should_poll = self.last_poll.lock().unwrap().elapsed() >= self.config.poll_interval;

        if should_poll {
            match self.poll_inbound().await {
                Ok(stats) => {
                    if stats.downloaded > 0 || stats.deleted > 0 {
                        log::info!(
                            "[sync] Inbound: {} downloaded, {} deleted",
                            stats.downloaded,
                            stats.deleted
                        );
                        did_work = true;
                    }
                }
                Err(e) => {
                    log::error!("[sync] Inbound poll error: {}", e);
                }
            }
            *self.last_poll.lock().unwrap() = Instant::now();
        }

        did_work
    }

    /// Force flush all pending outbound operations. Leases are not touched;
    /// use [`Self::release_all_leases`] for shutdown.
    pub async fn force_flush(&self) -> Result<usize, SyncError> {
        let mut total = 0;
        loop {
            let is_empty = self.outbound_queue.lock().unwrap().is_empty();
            if is_empty {
                break;
            }
            total += self.flush_outbound().await?;
        }
        Ok(total)
    }

    async fn flush_outbound(&self) -> Result<usize, SyncError> {
        let mut processed = 0;
        let batch_size = self.config.outbound_batch_size;
        let is_realtime = self.config.mode == SyncMode::RealTime;

        for _ in 0..batch_size {
            let op = self.outbound_queue.lock().unwrap().pop_front();

            match op {
                Some(SyncOperation::Upload { path }) => {
                    // Realtime mode: uploads are handled inline by hooks
                    // (`sync_file_now`); discard any stragglers in the queue.
                    if is_realtime {
                        continue;
                    }
                    // Leased paths are pushed on close.
                    if self.is_locked(&path) {
                        continue;
                    }
                    let cond = self.cond_for_upload(&path);
                    match self.upload_with_cond(&path, cond).await {
                        Ok(_) => processed += 1,
                        Err(SyncError::S3(e)) if e.is_precondition_failed() => {
                            match self.handle_upload_conflict(&path).await {
                                Ok(()) => processed += 1,
                                Err(SyncError::Conflict { .. }) => {
                                    // Remote won; the op is intentionally dropped.
                                    processed += 1;
                                }
                                Err(e) => {
                                    self.outbound_queue
                                        .lock()
                                        .unwrap()
                                        .push_back(SyncOperation::Upload { path });
                                    return Err(e);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("[sync] Failed to upload {}: {}", path, e);
                            self.outbound_queue
                                .lock()
                                .unwrap()
                                .push_back(SyncOperation::Upload { path });
                            return Err(e);
                        }
                    }
                }
                Some(SyncOperation::Delete { path, etag }) => {
                    self.flush_delete(&path, etag).await;
                    processed += 1;
                }
                None => break,
            }
        }

        *self.last_flush.lock().unwrap() = Instant::now();
        Ok(processed)
    }

    /// Delete `path` from S3 conditionally on `etag` (when known). A lost
    /// condition means someone replaced the object after we last saw it;
    /// with file locks enabled the remote version is restored locally.
    async fn flush_delete(&self, path: &str, etag: Option<String>) {
        let cond = match &etag {
            Some(e) => Precondition::IfMatch(e.clone()),
            None => Precondition::None,
        };
        match self.store.delete_file(path, cond).await {
            Ok(()) => log::info!("[sync] Deleted from S3: {}", path),
            Err(e) if e.is_precondition_failed() => {
                if self.config.file_lock {
                    log::warn!(
                        "[sync] {} changed in S3 before local delete; restoring the S3 version",
                        path
                    );
                    self.refresh_local_from_remote(path).await;
                } else {
                    log::warn!(
                        "[sync] {} changed in S3 before local delete; deleting anyway (file lock disabled)",
                        path
                    );
                    if let Err(e) = self.store.delete_file(path, Precondition::None).await {
                        log::error!("[sync] Failed to delete {}: {}", path, e);
                    }
                }
            }
            Err(e) if e.is_not_implemented() => {
                // Store could not honour the condition; a plain delete is the
                // closest available behaviour.
                if let Err(e) = self.store.delete_file(path, Precondition::None).await {
                    log::error!("[sync] Failed to delete {}: {}", path, e);
                }
            }
            Err(e) => log::error!("[sync] Failed to delete {}: {}", path, e),
        }
    }

    // -----------------------------------------------------------------
    // Inbound (polling)
    // -----------------------------------------------------------------

    fn read_file_content(&self, path: &str) -> Result<Vec<u8>, S3Error> {
        let fd = self.fs.open_read(path)?;
        let size = self.fs.fstat_size(fd)? as usize;
        let mut content = vec![0u8; size];
        self.fs.read(fd, &mut content)?;
        let _ = self.fs.close(fd);
        Ok(content)
    }

    fn has_pending_op(&self, path: &str) -> bool {
        self.outbound_queue
            .lock()
            .unwrap()
            .iter()
            .any(|op| op.path() == path)
    }

    async fn poll_inbound(&self) -> Result<SyncStats, S3Error> {
        let mut stats = SyncStats::default();

        let s3_objects = self.store.list_files().await?;
        let s3_files: HashSet<String> = s3_objects
            .iter()
            .filter(|o| !o.is_dir)
            .map(|o| o.path.clone())
            .collect();
        let s3_dirs: HashSet<String> = s3_objects
            .iter()
            .filter(|o| o.is_dir)
            .map(|o| o.path.clone())
            .collect();

        for obj in &s3_objects {
            if obj.is_dir {
                let known = self.metadata_cache.lock().unwrap().has_dir(&obj.path);
                if !known {
                    self.fs.mkdir_p(&obj.path);
                    self.metadata_cache
                        .lock()
                        .unwrap()
                        .add_dir(&obj.path, obj.etag.clone());
                    log::info!("[sync] Directory created from S3: {}", obj.path);
                }
                continue;
            }

            let should_download = {
                let cache = self.metadata_cache.lock().unwrap();
                match cache.get(&obj.path) {
                    Some(meta) => obj.etag != meta.etag,
                    None => true,
                }
            };

            if !should_download || self.is_locked(&obj.path) || self.has_pending_op(&obj.path) {
                continue;
            }

            if let Err(e) = self.download_file(obj).await {
                log::error!("[sync] Failed to download {}: {}", obj.path, e);
            } else {
                stats.downloaded += 1;
            }
        }

        let local_paths: Vec<String> = self
            .metadata_cache
            .lock()
            .unwrap()
            .paths()
            .cloned()
            .collect();

        for path in local_paths {
            if s3_files.contains(&path) || self.is_locked(&path) || self.has_pending_op(&path) {
                continue;
            }
            if let Err(e) = self.delete_local_file(&path) {
                log::error!("[sync] Failed to delete local {}: {}", path, e);
            } else {
                stats.deleted += 1;
            }
        }

        let local_dirs: Vec<String> = self
            .metadata_cache
            .lock()
            .unwrap()
            .dirs()
            .cloned()
            .collect();

        for dir in local_dirs {
            if s3_dirs.contains(&dir) {
                continue;
            }
            // Best effort: the directory may still hold local files.
            if self.fs.rmdir(&dir).is_ok() {
                log::info!("[sync] Directory removed (marker gone from S3): {}", dir);
            }
            self.metadata_cache.lock().unwrap().remove_dir(&dir);
        }

        Ok(stats)
    }

    async fn download_file(&self, obj: &S3ObjectInfo) -> Result<(), S3Error> {
        let (content, etag, last_modified) =
            self.store
                .get_file(&obj.path)
                .await?
                .ok_or_else(|| S3Error::NotFound {
                    key: obj.path.clone(),
                })?;

        self.write_file_content(&obj.path, &content)?;

        self.metadata_cache.lock().unwrap().update_after_download(
            &obj.path,
            etag,
            last_modified,
            content.len() as u64,
        );

        log::info!("[sync] Downloaded: {}", obj.path);
        Ok(())
    }

    fn write_file_content(&self, path: &str, content: &[u8]) -> Result<(), S3Error> {
        if let Some(parent) = Path::new(path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                self.fs.mkdir_p(&parent_str);
            }
        }

        let fd = self.fs.open_write_truncate(path)?;
        self.fs.write(fd, content)?;
        let _ = self.fs.close(fd);
        Ok(())
    }

    fn delete_local_file(&self, path: &str) -> Result<(), S3Error> {
        self.fs.unlink(path)?;
        self.metadata_cache.lock().unwrap().remove(path);
        log::info!("[sync] Deleted locally: {}", path);
        Ok(())
    }
}

/// Populate `fs` from S3 and return a fresh `MetadataCache`. Consumers
/// build their own `Fs` (with the appropriate `TimeProvider`) and wrap it
/// in an `FsBackend` impl before calling.
pub async fn populate_from_s3<F: FsBackend, S: ObjectStore>(
    store: &S,
    fs: &F,
) -> Result<MetadataCache, LoadError> {
    let mut cache = MetadataCache::new();

    let objects = store
        .list_files()
        .await
        .map_err(|e| LoadError::S3 { source: e })?;

    let file_count = objects.iter().filter(|o| !o.is_dir).count();
    log::info!("[sync] Found {} files in S3", file_count);

    for obj in objects.iter().filter(|o| o.is_dir) {
        fs.mkdir_p(&obj.path);
        cache.add_dir(&obj.path, obj.etag.clone());
        log::info!("[sync] Loaded directory: {}", obj.path);
    }

    for obj in objects.iter().filter(|o| !o.is_dir) {
        let (content, etag, last_modified) = match store.get_file(&obj.path).await {
            Ok(Some(data)) => data,
            Ok(None) => continue,
            Err(e) => {
                log::error!("[sync] Failed to download {}: {}", obj.path, e);
                continue;
            }
        };

        if let Some(parent) = Path::new(&obj.path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                fs.mkdir_p(&parent_str);
            }
        }

        let fd = fs
            .open_write_truncate(&obj.path)
            .map_err(|e| LoadError::Fs {
                message: format!("{:?}", e),
            })?;

        fs.write(fd, &content).map_err(|e| LoadError::Fs {
            message: format!("{:?}", e),
        })?;

        fs.close(fd).map_err(|e| LoadError::Fs {
            message: format!("{:?}", e),
        })?;

        cache.update_after_download(&obj.path, etag, last_modified, content.len() as u64);

        log::info!("[sync] Loaded: {}", obj.path);
    }

    Ok(cache)
}
