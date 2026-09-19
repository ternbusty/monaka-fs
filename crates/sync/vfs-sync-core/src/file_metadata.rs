//! File metadata cache for S3 synchronization
//!
//! Tracks the ETag this instance last saw for every synced file, which is
//! the sole input to inbound change detection and the base for conditional
//! writes. Directory markers are tracked separately.

use std::collections::HashMap;

/// Metadata for a synced file
#[derive(Debug, Clone)]
pub struct SyncedFileMetadata {
    /// S3 ETag (usually MD5 hash of content)
    pub etag: String,
    /// Last modified timestamp from S3 (Unix epoch seconds)
    pub last_modified: u64,
    /// Local modification timestamp (VFS modified time)
    pub local_modified: u64,
    /// File size in bytes
    pub size: u64,
}

/// Cache of synced file metadata
#[derive(Default)]
pub struct MetadataCache {
    /// Map from VFS path to sync metadata
    files: HashMap<String, SyncedFileMetadata>,
    /// Map from VFS directory path to the ETag of its marker object
    dirs: HashMap<String, String>,
}

impl MetadataCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update metadata for a file after successful S3 upload
    pub fn update_after_upload(
        &mut self,
        path: &str,
        etag: String,
        size: u64,
        local_modified: u64,
    ) {
        self.files.insert(
            path.to_string(),
            SyncedFileMetadata {
                etag,
                last_modified: current_timestamp(),
                local_modified,
                size,
            },
        );
    }

    /// Update metadata for a file after successful S3 download
    pub fn update_after_download(
        &mut self,
        path: &str,
        etag: String,
        last_modified: u64,
        size: u64,
    ) {
        self.files.insert(
            path.to_string(),
            SyncedFileMetadata {
                etag,
                last_modified,
                local_modified: last_modified,
                size,
            },
        );
    }

    /// Get metadata for a path
    pub fn get(&self, path: &str) -> Option<&SyncedFileMetadata> {
        self.files.get(path)
    }

    /// Remove metadata for a path
    pub fn remove(&mut self, path: &str) {
        self.files.remove(path);
    }

    /// Get all tracked file paths
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }

    pub fn add_dir(&mut self, path: &str, etag: String) {
        self.dirs.insert(path.to_string(), etag);
    }

    pub fn remove_dir(&mut self, path: &str) {
        self.dirs.remove(path);
    }

    pub fn dir_etag(&self, path: &str) -> Option<&String> {
        self.dirs.get(path)
    }

    pub fn has_dir(&self, path: &str) -> bool {
        self.dirs.contains_key(path)
    }

    /// Get all tracked directory paths
    pub fn dirs(&self) -> impl Iterator<Item = &String> {
        self.dirs.keys()
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
