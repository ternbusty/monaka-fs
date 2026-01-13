//! Sync Manager for S3 persistence

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

use tokio::time::{Duration, Instant};

use super::file_metadata::MetadataCache;
use super::s3_client::{S3Error, S3ObjectInfo, S3Storage};
use fs_core::Fs;

#[derive(Debug, Clone)]
pub enum SyncOperation {
    Upload { path: String },
    Delete { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    #[default]
    Batch,
    RealTime,
}

impl SyncMode {
    pub fn from_env() -> Self {
        match std::env::var("VFS_SYNC_MODE").as_deref() {
            Ok("realtime") | Ok("real-time") | Ok("immediate") => SyncMode::RealTime,
            _ => SyncMode::Batch,
        }
    }
}

pub struct SyncConfig {
    pub mode: SyncMode,
    pub poll_interval: Duration,
    pub flush_interval: Duration,
    pub outbound_batch_size: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            mode: SyncMode::from_env(),
            poll_interval: Duration::from_secs(30),
            flush_interval: Duration::from_secs(5),
            outbound_batch_size: 10,
        }
    }
}

impl SyncConfig {
    pub fn from_env() -> Self {
        Self {
            mode: SyncMode::from_env(),
            ..Default::default()
        }
    }
}

pub struct SyncManager<T: fs_core::TimeProvider> {
    s3: Rc<S3Storage>,
    fs: Rc<RefCell<Fs<T>>>,
    outbound_queue: RefCell<VecDeque<SyncOperation>>,
    metadata_cache: RefCell<MetadataCache>,
    config: SyncConfig,
    last_poll: RefCell<Instant>,
    last_flush: RefCell<Instant>,
}

impl<T: fs_core::TimeProvider> SyncManager<T> {
    pub fn new(
        s3: Rc<S3Storage>,
        fs: Rc<RefCell<Fs<T>>>,
        metadata_cache: MetadataCache,
        config: SyncConfig,
    ) -> Self {
        Self {
            s3,
            fs,
            outbound_queue: RefCell::new(VecDeque::new()),
            metadata_cache: RefCell::new(metadata_cache),
            config,
            last_poll: RefCell::new(Instant::now()),
            last_flush: RefCell::new(Instant::now()),
        }
    }

    pub fn enqueue_upload(&self, path: String) {
        let mut queue = self.outbound_queue.borrow_mut();
        queue.retain(|op| match op {
            SyncOperation::Upload { path: p } | SyncOperation::Delete { path: p } => p != &path,
        });
        queue.push_back(SyncOperation::Upload { path });
    }

    pub fn enqueue_delete(&self, path: String) {
        let mut queue = self.outbound_queue.borrow_mut();
        queue.retain(|op| match op {
            SyncOperation::Upload { path: p } | SyncOperation::Delete { path: p } => p != &path,
        });
        queue.push_back(SyncOperation::Delete { path: path.clone() });
        self.metadata_cache.borrow_mut().remove(&path);
    }

    pub fn pending_count(&self) -> usize {
        self.outbound_queue.borrow().len()
    }

    pub fn is_realtime(&self) -> bool {
        self.config.mode == SyncMode::RealTime
    }

    pub async fn sync_file_now(&self, path: &str) -> Result<(), S3Error> {
        self.upload_file(path).await
    }

    pub async fn maybe_sync(&self) -> bool {
        let mut did_work = false;

        let should_flush = {
            let queue_len = self.outbound_queue.borrow().len();
            let elapsed = self.last_flush.borrow().elapsed();

            match self.config.mode {
                SyncMode::RealTime => queue_len > 0,
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
                        did_work = true;
                    }
                }
                Err(e) => {
                    log::error!("[sync] Outbound flush error: {}", e);
                }
            }
        }

        let should_poll = self.last_poll.borrow().elapsed() >= self.config.poll_interval;

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
            *self.last_poll.borrow_mut() = Instant::now();
        }

        did_work
    }

    pub async fn force_flush(&self) -> Result<usize, S3Error> {
        let mut total = 0;
        while !self.outbound_queue.borrow().is_empty() {
            total += self.flush_outbound().await?;
        }
        Ok(total)
    }

    async fn flush_outbound(&self) -> Result<usize, S3Error> {
        let mut processed = 0;
        let batch_size = self.config.outbound_batch_size;

        for _ in 0..batch_size {
            let op = self.outbound_queue.borrow_mut().pop_front();

            match op {
                Some(SyncOperation::Upload { path }) => {
                    if let Err(e) = self.upload_file(&path).await {
                        log::error!("[sync] Failed to upload {}: {}", path, e);
                        self.outbound_queue
                            .borrow_mut()
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

        *self.last_flush.borrow_mut() = Instant::now();
        Ok(processed)
    }

    async fn upload_file(&self, path: &str) -> Result<(), S3Error> {
        let content = self.read_file_content(path)?;
        let size = content.len() as u64;
        let local_modified = self.fs.borrow().stat(path).map(|m| m.modified).unwrap_or(0);

        let etag = self.s3.put_file_with_etag(path, content).await?;

        self.metadata_cache
            .borrow_mut()
            .update_after_upload(path, etag, size, local_modified);

        log::info!("[sync] Uploaded: {}", path);
        Ok(())
    }

    fn read_file_content(&self, path: &str) -> Result<Vec<u8>, S3Error> {
        let mut fs = self.fs.borrow_mut();

        let fd = fs
            .open_path_with_flags(path, fs_core::O_RDONLY)
            .map_err(|e| S3Error::Read {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })?;

        let size = fs
            .fstat(fd)
            .map_err(|e| S3Error::Read {
                key: path.to_string(),
                message: format!("Failed to stat: {:?}", e),
            })?
            .size as usize;

        let mut content = vec![0u8; size];
        fs.read(fd, &mut content).map_err(|e| S3Error::Read {
            key: path.to_string(),
            message: format!("Failed to read: {:?}", e),
        })?;

        let _ = fs.close(fd);

        Ok(content)
    }

    async fn poll_inbound(&self) -> Result<SyncStats, S3Error> {
        let mut stats = SyncStats::default();

        let s3_objects = self.s3.list_objects().await?;
        let s3_paths: HashSet<String> = s3_objects.iter().map(|o| o.path.clone()).collect();

        for obj in s3_objects {
            let should_download = {
                let cache = self.metadata_cache.borrow();
                match cache.get(&obj.path) {
                    Some(meta) => obj.etag != meta.etag && obj.last_modified > meta.local_modified,
                    None => true,
                }
            };

            if should_download {
                let in_queue = self.outbound_queue.borrow().iter().any(|op| match op {
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

        let local_paths: Vec<String> = self.metadata_cache.borrow().paths().cloned().collect();

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

        if let Some(parent) = std::path::Path::new(&obj.path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                let _ = self.fs.borrow_mut().mkdir_p(&format!("/{}", parent_str));
            }
        }

        self.write_file_content(&obj.path, &content)?;

        self.metadata_cache.borrow_mut().update_after_download(
            &obj.path,
            etag,
            last_modified,
            content.len() as u64,
        );

        log::info!("[sync] Downloaded: {}", obj.path);
        Ok(())
    }

    fn write_file_content(&self, path: &str, content: &[u8]) -> Result<(), S3Error> {
        let mut fs = self.fs.borrow_mut();

        if let Some(parent) = std::path::Path::new(path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                let _ = fs.mkdir_p(&parent_str);
            }
        }

        let fd = fs
            .open_path_with_flags(path, fs_core::O_RDWR | fs_core::O_CREAT | fs_core::O_TRUNC)
            .map_err(|e| S3Error::Write {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })?;

        fs.write(fd, content).map_err(|e| S3Error::Write {
            key: path.to_string(),
            message: format!("Failed to write: {:?}", e),
        })?;

        let _ = fs.close(fd);

        Ok(())
    }

    fn delete_local_file(&self, path: &str) -> Result<(), S3Error> {
        self.fs
            .borrow_mut()
            .unlink(path)
            .map_err(|e| S3Error::Delete {
                key: path.to_string(),
                message: format!("Failed to unlink: {:?}", e),
            })?;

        self.metadata_cache.borrow_mut().remove(path);
        log::info!("[sync] Deleted locally: {}", path);
        Ok(())
    }
}

#[derive(Default)]
pub struct SyncStats {
    pub downloaded: usize,
    pub deleted: usize,
}
