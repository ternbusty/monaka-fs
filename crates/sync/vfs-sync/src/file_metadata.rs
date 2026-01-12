//! File metadata cache for S3 synchronization
//!
//! Tracks ETag and timestamps for synced files to enable
//! change detection and conflict resolution.

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
pub struct MetadataCache {
    /// Map from VFS path to sync metadata
    files: HashMap<String, SyncedFileMetadata>,
}

impl MetadataCache {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
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

    /// Get all tracked paths
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }
}

impl Default for MetadataCache {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
