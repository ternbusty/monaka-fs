//! File metadata cache for S3 synchronization

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SyncedFileMetadata {
    pub etag: String,
    pub last_modified: u64,
    pub local_modified: u64,
    pub size: u64,
}

pub struct MetadataCache {
    files: HashMap<String, SyncedFileMetadata>,
}

impl MetadataCache {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

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

    pub fn get(&self, path: &str) -> Option<&SyncedFileMetadata> {
        self.files.get(path)
    }

    pub fn remove(&mut self, path: &str) {
        self.files.remove(path);
    }

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
