//! Sync Manager for S3 persistence
//!
//! Manages dirty tracking and synchronization to S3.
//! Uses a single-threaded design compatible with WASI.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use tokio::time::{Duration, Instant};

use crate::s3_client::{S3Error, S3Storage};
use fs_core::snapshot::FsSnapshot;
use fs_core::Fs;

/// Sync job types
#[derive(Debug, Clone)]
pub enum SyncJob {
    /// Sync a single file path
    File { path: String },
    /// Delete a file from S3
    Delete { path: String },
    /// Save full snapshot
    Snapshot,
}

/// Sync manager configuration
pub struct SyncConfig {
    /// Interval for automatic snapshot saves
    pub snapshot_interval: Duration,
    /// Maximum number of dirty entries before forcing a snapshot
    pub max_dirty_entries: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            snapshot_interval: Duration::from_secs(60),
            max_dirty_entries: 100,
        }
    }
}

/// Manages dirty tracking and S3 synchronization
///
/// This is designed for single-threaded WASI environment.
/// Call `maybe_sync()` periodically from the main loop.
pub struct SyncManager {
    /// S3 storage client
    s3: Rc<S3Storage>,
    /// Reference to the filesystem
    fs: Rc<RefCell<Fs>>,
    /// Set of dirty file paths
    dirty_set: RefCell<HashSet<String>>,
    /// Pending delete paths
    pending_deletes: RefCell<Vec<String>>,
    /// Configuration
    config: SyncConfig,
    /// Last snapshot time
    last_snapshot: RefCell<Instant>,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(s3: Rc<S3Storage>, fs: Rc<RefCell<Fs>>, config: SyncConfig) -> Self {
        Self {
            s3,
            fs,
            dirty_set: RefCell::new(HashSet::new()),
            pending_deletes: RefCell::new(Vec::new()),
            config,
            last_snapshot: RefCell::new(Instant::now()),
        }
    }

    /// Mark a path as dirty (needs sync to S3)
    pub fn mark_dirty(&self, path: String) {
        self.dirty_set.borrow_mut().insert(path);
    }

    /// Mark a path for deletion
    pub fn mark_deleted(&self, path: String) {
        self.dirty_set.borrow_mut().remove(&path);
        self.pending_deletes.borrow_mut().push(path);
    }

    /// Check if sync is needed and perform it
    ///
    /// Call this periodically from the main loop.
    /// Returns true if a snapshot was saved.
    pub async fn maybe_sync(&self) -> bool {
        // Process pending deletes
        let deletes: Vec<String> = self.pending_deletes.borrow_mut().drain(..).collect();
        for path in deletes {
            if let Err(e) = self.s3.delete_file(&path).await {
                eprintln!("[sync] Failed to delete {}: {}", path, e);
            } else {
                println!("[sync] Deleted from S3: {}", path);
            }
        }

        // Check if we need to save a snapshot
        let dirty_count = self.dirty_set.borrow().len();
        let elapsed = self.last_snapshot.borrow().elapsed();

        let should_snapshot = dirty_count > 0
            && (dirty_count >= self.config.max_dirty_entries
                || elapsed >= self.config.snapshot_interval);

        if should_snapshot {
            match self.save_snapshot().await {
                Ok(()) => {
                    *self.last_snapshot.borrow_mut() = Instant::now();
                    return true;
                }
                Err(e) => {
                    eprintln!("[sync] Snapshot failed: {}", e);
                }
            }
        }

        false
    }

    /// Force a snapshot save
    pub async fn force_snapshot(&self) -> Result<(), S3Error> {
        self.save_snapshot().await?;
        *self.last_snapshot.borrow_mut() = Instant::now();
        Ok(())
    }

    /// Get the number of dirty paths
    pub fn dirty_count(&self) -> usize {
        self.dirty_set.borrow().len()
    }

    /// Save snapshot to S3
    async fn save_snapshot(&self) -> Result<(), S3Error> {
        // Take a snapshot of the current filesystem state
        let snapshot = self.fs.borrow().to_snapshot();

        // Serialize to JSON
        let json = serde_json::to_vec(&snapshot).map_err(|e| S3Error::Write {
            key: "snapshot.json".to_string(),
            message: format!("Serialization error: {}", e),
        })?;

        // Upload to S3
        self.s3.save_snapshot(&json).await?;

        // Clear dirty set after successful snapshot
        self.dirty_set.borrow_mut().clear();

        println!("[sync] Snapshot saved successfully ({} bytes)", json.len());
        Ok(())
    }
}

/// Load filesystem from S3 snapshot
pub async fn load_from_s3(s3: &S3Storage) -> Result<Option<Fs>, LoadError> {
    match s3.load_snapshot().await {
        Ok(Some(data)) => {
            let snapshot: FsSnapshot =
                serde_json::from_slice(&data).map_err(|e| LoadError::Deserialize {
                    message: e.to_string(),
                })?;

            let fs = Fs::from_snapshot(snapshot, fs_core::MonotonicCounter::new());
            println!(
                "[sync] Loaded filesystem from S3 snapshot ({} bytes)",
                data.len()
            );
            Ok(Some(fs))
        }
        Ok(None) => {
            println!("[sync] No existing snapshot found, starting with empty filesystem");
            Ok(None)
        }
        Err(e) => Err(LoadError::S3 { source: e }),
    }
}

/// Error loading filesystem from S3
#[derive(Debug)]
pub enum LoadError {
    S3 { source: S3Error },
    Deserialize { message: String },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::S3 { source } => write!(f, "S3 error: {}", source),
            LoadError::Deserialize { message } => write!(f, "Deserialization error: {}", message),
        }
    }
}

impl std::error::Error for LoadError {}
