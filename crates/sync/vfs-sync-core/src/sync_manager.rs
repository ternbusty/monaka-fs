//! Bidirectional S3 sync manager (host- and WASI-shared).
//!
//! Generic over [`FsBackend`] so that both the multi-threaded native host
//! (`Arc<fs_core::Fs>`) and the single-threaded WASI adapter
//! (`Rc<RefCell<fs_core::Fs<T>>>`) can share the same logic. State is kept
//! behind `std::sync::Mutex`, which is correct in both cases (single-thread
//! WASI never contends).

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::time::Instant;

use crate::config::{MetadataMode, SyncConfig, SyncMode, SyncOperation};
use crate::file_metadata::MetadataCache;
use crate::fs_backend::FsBackend;
use crate::s3_client::S3Storage;
use crate::types::{S3Error, S3ObjectInfo};

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
/// The `Send + Sync` story is determined by the chosen `F`. With
/// `F = Arc<Fs>` the manager is `Send + Sync` and can be shared via
/// `Arc<SyncManager<…>>` across threads (host); with
/// `F = Rc<RefCell<Fs<T>>>` it is `!Send`, matching single-threaded WASI use.
pub struct SyncManager<F: FsBackend> {
    s3: Arc<S3Storage>,
    fs: F,
    outbound_queue: Mutex<VecDeque<SyncOperation>>,
    metadata_cache: Mutex<MetadataCache>,
    config: SyncConfig,
    last_poll: Mutex<Instant>,
    last_flush: Mutex<Instant>,
    shutdown: AtomicBool,
}

impl<F: FsBackend> SyncManager<F> {
    /// Create a new sync manager.
    pub fn new(
        s3: Arc<S3Storage>,
        fs: F,
        metadata_cache: MetadataCache,
        config: SyncConfig,
    ) -> Self {
        Self {
            s3,
            fs,
            outbound_queue: Mutex::new(VecDeque::new()),
            metadata_cache: Mutex::new(metadata_cache),
            config,
            last_poll: Mutex::new(Instant::now()),
            last_flush: Mutex::new(Instant::now()),
            shutdown: AtomicBool::new(false),
        }
    }

    /// Enqueue a file upload (deduping any prior op for the same path).
    pub fn enqueue_upload(&self, path: String) {
        let mut queue = self.outbound_queue.lock().unwrap();

        queue.retain(|op| match op {
            SyncOperation::Upload { path: p } | SyncOperation::Delete { path: p } => p != &path,
        });

        queue.push_back(SyncOperation::Upload { path });
    }

    /// Enqueue a file deletion (also drops any cached metadata).
    pub fn enqueue_delete(&self, path: String) {
        let mut queue = self.outbound_queue.lock().unwrap();

        queue.retain(|op| match op {
            SyncOperation::Upload { path: p } | SyncOperation::Delete { path: p } => p != &path,
        });

        queue.push_back(SyncOperation::Delete { path: path.clone() });
        self.metadata_cache.lock().unwrap().remove(&path);
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

    /// Synchronously upload a single file to S3 (used by realtime hooks).
    pub async fn sync_file_now(&self, path: &str) -> Result<(), S3Error> {
        self.upload_file(path).await
    }

    /// Refresh a single file from S3 (read-through mode).
    pub async fn refresh_file_from_s3(&self, path: &str) -> Result<(), S3Error> {
        if let Some((content, etag, last_modified)) = self.s3.get_file(path).await? {
            self.write_file_content(path, &content)?;
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
    /// Returns `true` if the file was refreshed.
    pub async fn check_and_refresh_from_s3(&self, path: &str) -> Result<bool, S3Error> {
        let s3_meta = match self.s3.head_file(path).await? {
            Some(meta) => meta,
            None => {
                log::debug!("[sync] File not found in S3: {}", path);
                return Ok(false);
            }
        };

        let (s3_etag, _s3_last_modified, _s3_size) = s3_meta;

        let needs_refresh = {
            let cache = self.metadata_cache.lock().unwrap();
            match cache.get(path) {
                Some(local_meta) => s3_etag != local_meta.etag,
                None => true,
            }
        };

        if needs_refresh {
            if let Some((content, etag, last_modified)) = self.s3.get_file(path).await? {
                self.write_file_content(path, &content)?;
                self.metadata_cache.lock().unwrap().update_after_download(
                    path,
                    etag,
                    last_modified,
                    content.len() as u64,
                );
                log::debug!("[sync] Refreshed from S3 (metadata changed): {}", path);
                return Ok(true);
            }
        } else {
            log::debug!("[sync] S3 metadata unchanged for: {}", path);
        }

        Ok(false)
    }

    /// Cooperative sync check - call from background thread / event loop.
    pub async fn maybe_sync(&self) -> bool {
        if self.is_shutdown() {
            return false;
        }

        let mut did_work = false;

        // In RealTime mode, uploads are handled inline by `sync_file_now` hooks,
        // so the background flush only handles pending Delete ops.
        let should_flush = {
            let queue_len = self.outbound_queue.lock().unwrap().len();
            let elapsed = self.last_flush.lock().unwrap().elapsed();

            match self.config.mode {
                SyncMode::RealTime => self
                    .outbound_queue
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|op| matches!(op, SyncOperation::Delete { .. })),
                SyncMode::Batch => {
                    queue_len >= self.config.outbound_batch_size
                        || (queue_len > 0 && elapsed >= self.config.flush_interval)
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

    /// Force flush all pending outbound operations.
    pub async fn force_flush(&self) -> Result<usize, S3Error> {
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

    async fn flush_outbound(&self) -> Result<usize, S3Error> {
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
                    if let Err(e) = self.upload_file(&path).await {
                        log::error!("[sync] Failed to upload {}: {}", path, e);
                        self.outbound_queue
                            .lock()
                            .unwrap()
                            .push_back(SyncOperation::Upload { path });
                        return Err(e);
                    }
                    processed += 1;
                }
                Some(SyncOperation::Delete { path }) => {
                    if let Err(e) = self.s3.delete_file(&path).await {
                        log::error!("[sync] Failed to delete {}: {}", path, e);
                    } else {
                        log::info!("[sync] Deleted from S3: {}", path);
                    }
                    processed += 1;
                }
                None => break,
            }
        }

        *self.last_flush.lock().unwrap() = Instant::now();
        Ok(processed)
    }

    async fn upload_file(&self, path: &str) -> Result<(), S3Error> {
        let content = self.read_file_content(path)?;
        let size = content.len() as u64;
        let local_modified = self.fs.stat_modified(path);

        // S3 passthrough mode: pre-write existence checks (matches s3fs-fuse).
        if self.config.metadata_mode == MetadataMode::S3 {
            let (file_check, dir_check, children_check) = tokio::join!(
                self.s3.head_file(path),
                self.s3.head_directory_object(path),
                self.s3.has_children(path),
            );
            file_check?;
            dir_check?;
            children_check?;
        }

        let etag = self.s3.put_file_with_etag(path, content).await?;

        self.metadata_cache
            .lock()
            .unwrap()
            .update_after_upload(path, etag, size, local_modified);

        log::info!("[sync] Uploaded: {}", path);
        Ok(())
    }

    fn read_file_content(&self, path: &str) -> Result<Vec<u8>, S3Error> {
        let fd = self.fs.open_read(path)?;
        let size = self.fs.fstat_size(fd)? as usize;
        let mut content = vec![0u8; size];
        self.fs.read(fd, &mut content)?;
        let _ = self.fs.close(fd);
        Ok(content)
    }

    async fn poll_inbound(&self) -> Result<SyncStats, S3Error> {
        let mut stats = SyncStats::default();

        let s3_objects = self.s3.list_objects().await?;
        let s3_paths: HashSet<String> = s3_objects.iter().map(|o| o.path.clone()).collect();

        for obj in s3_objects {
            let should_download = {
                let cache = self.metadata_cache.lock().unwrap();
                match cache.get(&obj.path) {
                    Some(meta) => obj.etag != meta.etag && obj.last_modified > meta.local_modified,
                    None => true,
                }
            };

            if should_download {
                // Skip if path is in outbound queue (local change pending)
                let in_queue = self
                    .outbound_queue
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|op| match op {
                        SyncOperation::Upload { path } => path == &obj.path,
                        _ => false,
                    });

                if !in_queue {
                    if let Err(e) = self.download_file(&obj).await {
                        log::error!("[sync] Failed to download {}: {}", obj.path, e);
                    } else {
                        stats.downloaded += 1;
                    }
                }
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
            if !s3_paths.contains(&path) {
                if let Err(e) = self.delete_local_file(&path) {
                    log::error!("[sync] Failed to delete local {}: {}", path, e);
                } else {
                    stats.deleted += 1;
                }
            }
        }

        Ok(stats)
    }

    async fn download_file(&self, obj: &S3ObjectInfo) -> Result<(), S3Error> {
        let (content, etag, last_modified) =
            self.s3
                .get_file(&obj.path)
                .await?
                .ok_or_else(|| S3Error::Read {
                    key: obj.path.clone(),
                    message: "File not found".to_string(),
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
pub async fn populate_from_s3<F: FsBackend>(
    s3: &S3Storage,
    fs: &F,
) -> Result<MetadataCache, LoadError> {
    let mut cache = MetadataCache::new();

    let objects = s3
        .list_objects()
        .await
        .map_err(|e| LoadError::S3 { source: e })?;

    log::info!("[sync] Found {} files in S3", objects.len());

    for obj in objects {
        let (content, etag, last_modified) = match s3.get_file(&obj.path).await {
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
