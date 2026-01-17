use alloc::{collections::BTreeMap, string::String};

use crate::storage::BlockStorage;
use crate::types::InodeId;

#[derive(Clone, Copy, Default)]
pub struct Metadata {
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub permissions: u16,
    pub is_dir: bool,
}

/// File content: either file data or directory entries
pub enum FileContent {
    File(BlockStorage),
    Dir(BTreeMap<String, InodeId>),
}

/// Inode structure representing a file or directory
pub struct Inode {
    pub id: InodeId,
    pub metadata: Metadata,
    pub content: FileContent,
}

impl Inode {
    pub fn new_file(id: InodeId) -> Self {
        Self {
            id,
            metadata: Metadata {
                size: 0,
                created: 0,
                modified: 0,
                permissions: 0o644,
                is_dir: false,
            },
            content: FileContent::File(BlockStorage::new()),
        }
    }

    pub fn new_dir(id: InodeId) -> Self {
        Self {
            id,
            metadata: Metadata {
                size: 0,
                created: 0,
                modified: 0,
                permissions: 0o755,
                is_dir: true,
            },
            content: FileContent::Dir(BTreeMap::new()),
        }
    }
}
