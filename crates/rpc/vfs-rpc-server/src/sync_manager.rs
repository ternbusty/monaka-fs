//! Sync Manager for S3 persistence
//!
//! Manages bidirectional file synchronization between VFS and S3.
//! Uses queue-based async sync for outbound and polling for inbound.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

use tokio::time::{Duration, Instant};

use crate::file_metadata::MetadataCache;
use crate::s3_client::{S3Error, S3ObjectInfo, S3Storage};
use fs_core::Fs;

/// Pending sync operation
#[derive(Debug, Clone)]
pub enum SyncOperation {
    /// Upload file to S3
    Upload { path: String },
    /// Delete file from S3
    Delete { path: String },
}

/// Sync manager configuration
pub struct SyncConfig {
    /// Interval for S3 polling (inbound)
    pub poll_interval: Duration,
    /// Interval for outbound queue flush
    pub flush_interval: Duration,
    /// Maximum operations per outbound flush
    pub outbound_batch_size: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(30),
            flush_interval: Duration::from_secs(5),
            outbound_batch_size: 10,
        }
    }
}

/// Manages bidirectional S3 synchronization
///
/// Designed for single-threaded WASI environment.
/// Call `maybe_sync()` periodically from the main loop.
pub struct SyncManager {
    /// S3 storage client
    s3: Rc<S3Storage>,
    /// Reference to the filesystem
    fs: Rc<RefCell<Fs>>,
    /// Pending outbound operations (VFS -> S3)
    outbound_queue: RefCell<VecDeque<SyncOperation>>,
    /// Metadata cache for conflict detection
    metadata_cache: RefCell<MetadataCache>,
    /// Configuration
    config: SyncConfig,
    /// Last poll time for inbound sync
    last_poll: RefCell<Instant>,
    /// Last outbound flush time
    last_flush: RefCell<Instant>,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(
        s3: Rc<S3Storage>,
        fs: Rc<RefCell<Fs>>,
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

    /// Enqueue a file upload
    pub fn enqueue_upload(&self, path: String) {
        let mut queue = self.outbound_queue.borrow_mut();

        // Remove any existing operation for this path (dedup)
        queue.retain(|op| match op {
            SyncOperation::Upload { path: p } | SyncOperation::Delete { path: p } => p != &path,
        });

        queue.push_back(SyncOperation::Upload { path });
    }

    /// Enqueue a file deletion
    pub fn enqueue_delete(&self, path: String) {
        let mut queue = self.outbound_queue.borrow_mut();

        // Remove any pending operation for this path
        queue.retain(|op| match op {
            SyncOperation::Upload { path: p } | SyncOperation::Delete { path: p } => p != &path,
        });

        queue.push_back(SyncOperation::Delete { path: path.clone() });

        // Also remove from metadata cache
        self.metadata_cache.borrow_mut().remove(&path);
    }

    /// Get the number of pending outbound operations
    pub fn pending_count(&self) -> usize {
        self.outbound_queue.borrow().len()
    }

    /// Cooperative sync check - call from main event loop
    pub async fn maybe_sync(&self) -> bool {
        let mut did_work = false;

        // Check if outbound flush is needed
        let should_flush = {
            let queue_len = self.outbound_queue.borrow().len();
            let elapsed = self.last_flush.borrow().elapsed();

            queue_len >= self.config.outbound_batch_size
                || (queue_len > 0 && elapsed >= self.config.flush_interval)
        };

        if should_flush {
            match self.flush_outbound().await {
                Ok(count) => {
                    if count > 0 {
                        println!("[sync] Flushed {} operations to S3", count);
                        did_work = true;
                    }
                }
                Err(e) => {
                    eprintln!("[sync] Outbound flush error: {}", e);
                }
            }
        }

        // Check if inbound poll is needed
        let should_poll = self.last_poll.borrow().elapsed() >= self.config.poll_interval;

        if should_poll {
            match self.poll_inbound().await {
                Ok(stats) => {
                    if stats.downloaded > 0 || stats.deleted > 0 {
                        println!(
                            "[sync] Inbound: {} downloaded, {} deleted",
                            stats.downloaded, stats.deleted
                        );
                        did_work = true;
                    }
                }
                Err(e) => {
                    eprintln!("[sync] Inbound poll error: {}", e);
                }
            }
            *self.last_poll.borrow_mut() = Instant::now();
        }

        did_work
    }

    /// Force flush all pending outbound operations
    pub async fn force_flush(&self) -> Result<usize, S3Error> {
        let mut total = 0;
        while !self.outbound_queue.borrow().is_empty() {
            total += self.flush_outbound().await?;
        }
        Ok(total)
    }

    /// Process outbound queue
    async fn flush_outbound(&self) -> Result<usize, S3Error> {
        let mut processed = 0;
        let batch_size = self.config.outbound_batch_size;

        for _ in 0..batch_size {
            let op = self.outbound_queue.borrow_mut().pop_front();

            match op {
                Some(SyncOperation::Upload { path }) => {
                    if let Err(e) = self.upload_file(&path).await {
                        eprintln!("[sync] Failed to upload {}: {}", path, e);
                        // Re-queue failed upload
                        self.outbound_queue
                            .borrow_mut()
                            .push_back(SyncOperation::Upload { path });
                        return Err(e);
                    }
                    processed += 1;
                }
                Some(SyncOperation::Delete { path }) => {
                    if let Err(e) = self.s3.delete_file(&path).await {
                        eprintln!("[sync] Failed to delete {}: {}", path, e);
                    } else {
                        println!("[sync] Deleted from S3: {}", path);
                    }
                    processed += 1;
                }
                None => break,
            }
        }

        *self.last_flush.borrow_mut() = Instant::now();
        Ok(processed)
    }

    /// Upload a single file to S3
    async fn upload_file(&self, path: &str) -> Result<(), S3Error> {
        // Read file content from VFS
        let content = self.read_file_content(path)?;
        let size = content.len() as u64;

        // Get local modification time
        let local_modified = self.fs.borrow().stat(path).map(|m| m.modified).unwrap_or(0);

        // Upload to S3
        let etag = self.s3.put_file_with_etag(path, content).await?;

        // Update metadata cache
        self.metadata_cache
            .borrow_mut()
            .update_after_upload(path, etag, size, local_modified);

        println!("[sync] Uploaded: {}", path);
        Ok(())
    }

    /// Read file content from VFS
    fn read_file_content(&self, path: &str) -> Result<Vec<u8>, S3Error> {
        let mut fs = self.fs.borrow_mut();

        // Open file for reading
        let fd = fs
            .open_path_with_flags(path, fs_core::O_RDONLY)
            .map_err(|e| S3Error::Read {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })?;

        // Get file size
        let size = fs
            .fstat(fd)
            .map_err(|e| S3Error::Read {
                key: path.to_string(),
                message: format!("Failed to stat: {:?}", e),
            })?
            .size as usize;

        // Read content
        let mut content = vec![0u8; size];
        fs.read(fd, &mut content).map_err(|e| S3Error::Read {
            key: path.to_string(),
            message: format!("Failed to read: {:?}", e),
        })?;

        // Close file
        let _ = fs.close(fd);

        Ok(content)
    }

    /// Poll S3 for changes and apply to VFS
    async fn poll_inbound(&self) -> Result<SyncStats, S3Error> {
        let mut stats = SyncStats::default();

        // List all objects in S3
        let s3_objects = self.s3.list_objects().await?;

        // Build set of S3 paths for deletion detection
        let s3_paths: HashSet<String> = s3_objects.iter().map(|o| o.path.clone()).collect();

        // Check each S3 object
        for obj in s3_objects {
            let should_download = {
                let cache = self.metadata_cache.borrow();
                match cache.get(&obj.path) {
                    Some(meta) => {
                        // Check if S3 has newer version
                        obj.etag != meta.etag && obj.last_modified > meta.local_modified
                    }
                    None => true, // New file, download it
                }
            };

            if should_download {
                // Skip if path is in outbound queue (local change pending)
                let in_queue = self.outbound_queue.borrow().iter().any(|op| match op {
                    SyncOperation::Upload { path } => path == &obj.path,
                    _ => false,
                });

                if !in_queue {
                    if let Err(e) = self.download_file(&obj).await {
                        eprintln!("[sync] Failed to download {}: {}", obj.path, e);
                    } else {
                        stats.downloaded += 1;
                    }
                }
            }
        }

        // Detect deleted files (in local cache but not in S3)
        let local_paths: Vec<String> = self.metadata_cache.borrow().paths().cloned().collect();

        for path in local_paths {
            if !s3_paths.contains(&path) {
                // File deleted from S3, remove from VFS
                if let Err(e) = self.delete_local_file(&path) {
                    eprintln!("[sync] Failed to delete local {}: {}", path, e);
                } else {
                    stats.deleted += 1;
                }
            }
        }

        Ok(stats)
    }

    /// Download a file from S3 to VFS
    async fn download_file(&self, obj: &S3ObjectInfo) -> Result<(), S3Error> {
        // Get file content from S3
        let (content, etag, last_modified) =
            self.s3
                .get_file(&obj.path)
                .await?
                .ok_or_else(|| S3Error::Read {
                    key: obj.path.clone(),
                    message: "File not found".to_string(),
                })?;

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(&obj.path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                let _ = self.fs.borrow_mut().mkdir_p(&format!("/{}", parent_str));
            }
        }

        // Write to VFS
        self.write_file_content(&obj.path, &content)?;

        // Update metadata cache
        self.metadata_cache.borrow_mut().update_after_download(
            &obj.path,
            etag,
            last_modified,
            content.len() as u64,
        );

        println!("[sync] Downloaded: {}", obj.path);
        Ok(())
    }

    /// Write content to VFS
    fn write_file_content(&self, path: &str, content: &[u8]) -> Result<(), S3Error> {
        let mut fs = self.fs.borrow_mut();

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                let _ = fs.mkdir_p(&parent_str);
            }
        }

        // Open file for writing (create if needed, truncate)
        let fd = fs
            .open_path_with_flags(path, fs_core::O_RDWR | fs_core::O_CREAT | fs_core::O_TRUNC)
            .map_err(|e| S3Error::Write {
                key: path.to_string(),
                message: format!("Failed to open: {:?}", e),
            })?;

        // Write content
        fs.write(fd, content).map_err(|e| S3Error::Write {
            key: path.to_string(),
            message: format!("Failed to write: {:?}", e),
        })?;

        // Close file
        let _ = fs.close(fd);

        Ok(())
    }

    /// Delete a file from VFS
    fn delete_local_file(&self, path: &str) -> Result<(), S3Error> {
        self.fs
            .borrow_mut()
            .unlink(path)
            .map_err(|e| S3Error::Delete {
                key: path.to_string(),
                message: format!("Failed to unlink: {:?}", e),
            })?;

        self.metadata_cache.borrow_mut().remove(path);
        println!("[sync] Deleted locally: {}", path);
        Ok(())
    }
}

/// Statistics from inbound sync
#[derive(Default)]
pub struct SyncStats {
    pub downloaded: usize,
    pub deleted: usize,
}

/// Initialize filesystem from S3
pub async fn init_from_s3(s3: &S3Storage) -> Result<(Fs, MetadataCache), LoadError> {
    let mut fs = Fs::new();
    let mut cache = MetadataCache::new();

    // List all files in S3
    let objects = s3
        .list_objects()
        .await
        .map_err(|e| LoadError::S3 { source: e })?;

    println!("[sync] Found {} files in S3", objects.len());

    for obj in objects {
        // Get file content
        let (content, etag, last_modified) = match s3.get_file(&obj.path).await {
            Ok(Some(data)) => data,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("[sync] Failed to download {}: {}", obj.path, e);
                continue;
            }
        };

        // Ensure parent directories exist
        if let Some(parent) = std::path::Path::new(&obj.path).parent() {
            let parent_str = parent.to_string_lossy();
            if !parent_str.is_empty() && parent_str != "/" {
                let _ = fs.mkdir_p(&parent_str);
            }
        }

        // Create file in VFS
        let fd = fs
            .open_path_with_flags(
                &obj.path,
                fs_core::O_RDWR | fs_core::O_CREAT | fs_core::O_TRUNC,
            )
            .map_err(|e| LoadError::Fs {
                message: format!("{:?}", e),
            })?;

        fs.write(fd, &content).map_err(|e| LoadError::Fs {
            message: format!("{:?}", e),
        })?;

        fs.close(fd).map_err(|e| LoadError::Fs {
            message: format!("{:?}", e),
        })?;

        // Update metadata cache
        cache.update_after_download(&obj.path, etag, last_modified, content.len() as u64);

        println!("[sync] Loaded: {}", obj.path);
    }

    Ok((fs, cache))
}

/// Error loading filesystem from S3
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
