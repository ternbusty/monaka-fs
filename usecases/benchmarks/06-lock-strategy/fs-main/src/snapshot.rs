//! Snapshot serialization/deserialization for filesystem persistence
//!
//! This module provides types and functions for serializing the filesystem
//! state to a snapshot format that can be stored externally (e.g., S3).

use alloc::{collections::BTreeMap, rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::fs::Fs;
use crate::inode::{FileContent, Inode, Metadata};
use crate::storage::BlockStorage;
use crate::time::TimeProvider;
use crate::types::InodeId;

/// Snapshot of the entire filesystem state
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FsSnapshot {
    /// Next inode ID to allocate
    pub next_inode: InodeId,
    /// Root inode ID
    pub root_inode: InodeId,
    /// All inodes in the filesystem
    pub inodes: Vec<InodeSnapshot>,
}

/// Snapshot of a single inode
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InodeSnapshot {
    pub id: InodeId,
    pub metadata: MetadataSnapshot,
    pub content: FileContentSnapshot,
}

/// Snapshot of file/directory metadata
#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MetadataSnapshot {
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub permissions: u16,
    pub is_dir: bool,
}

/// Snapshot of file content
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FileContentSnapshot {
    /// Regular file with its data
    File(FileDataSnapshot),
    /// Directory with name -> inode_id mappings
    Dir(BTreeMap<String, InodeId>),
}

/// Snapshot of file data (block storage)
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FileDataSnapshot {
    /// File size in bytes
    pub size: usize,
    /// File data as a contiguous byte array
    /// (sparse blocks are materialized as zeros)
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    pub data: Vec<u8>,
}

impl From<&Metadata> for MetadataSnapshot {
    fn from(m: &Metadata) -> Self {
        Self {
            size: m.size,
            created: m.created,
            modified: m.modified,
            permissions: m.permissions,
            is_dir: m.is_dir,
        }
    }
}

impl From<&MetadataSnapshot> for Metadata {
    fn from(m: &MetadataSnapshot) -> Self {
        Self {
            size: m.size,
            created: m.created,
            modified: m.modified,
            permissions: m.permissions,
            is_dir: m.is_dir,
        }
    }
}

impl From<&BlockStorage> for FileDataSnapshot {
    fn from(storage: &BlockStorage) -> Self {
        let size = storage.size();
        let mut data = vec![0u8; size];
        storage.read(0, &mut data);
        Self { size, data }
    }
}

impl<T: TimeProvider> Fs<T> {
    /// Create a snapshot of the current filesystem state
    pub fn to_snapshot(&self) -> FsSnapshot {
        let inodes: Vec<InodeSnapshot> = self
            .inode_table
            .iter()
            .map(|(&id, inode_rc)| {
                let inode = inode_rc.borrow();
                InodeSnapshot {
                    id,
                    metadata: MetadataSnapshot::from(&inode.metadata),
                    content: match &inode.content {
                        FileContent::File(storage) => {
                            FileContentSnapshot::File(FileDataSnapshot::from(storage))
                        }
                        FileContent::Dir(entries) => FileContentSnapshot::Dir(entries.clone()),
                    },
                }
            })
            .collect();

        FsSnapshot {
            next_inode: self.next_inode,
            root_inode: self.root_inode,
            inodes,
        }
    }

    /// Restore filesystem state from a snapshot
    pub fn from_snapshot(snapshot: FsSnapshot, time_provider: T) -> Self {
        let mut inode_table: BTreeMap<InodeId, Rc<RefCell<Inode>>> = BTreeMap::new();

        for inode_snap in snapshot.inodes {
            let content = match inode_snap.content {
                FileContentSnapshot::File(file_data) => {
                    let mut storage = BlockStorage::new();
                    if !file_data.data.is_empty() {
                        storage.write(0, &file_data.data);
                    }
                    // Ensure size is correct (handles sparse files)
                    if storage.size() != file_data.size {
                        storage.truncate(file_data.size);
                    }
                    FileContent::File(storage)
                }
                FileContentSnapshot::Dir(entries) => FileContent::Dir(entries),
            };

            let inode = Inode {
                id: inode_snap.id,
                metadata: Metadata::from(&inode_snap.metadata),
                content,
            };

            inode_table.insert(inode_snap.id, Rc::new(RefCell::new(inode)));
        }

        Self {
            next_inode: snapshot.next_inode,
            fd_table: BTreeMap::new(), // Start with empty fd_table
            inode_table,
            root_inode: snapshot.root_inode,
            time_provider,
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;
    use crate::time::MonotonicCounter;

    #[test]
    fn test_snapshot_roundtrip() {
        let mut fs = Fs::new();

        // Create some files and directories
        fs.mkdir("/test").unwrap();
        fs.mkdir_p("/test/nested/dir").unwrap();

        let fd = fs.open_path("/test/file.txt").unwrap();
        fs.write(fd, b"Hello, World!").unwrap();
        fs.close(fd).unwrap();

        // Create snapshot
        let snapshot = fs.to_snapshot();

        // Serialize to JSON
        let json = serde_json::to_string(&snapshot).unwrap();

        // Deserialize
        let restored_snapshot: FsSnapshot = serde_json::from_str(&json).unwrap();

        // Restore filesystem
        let mut restored_fs = Fs::from_snapshot(restored_snapshot, MonotonicCounter::new());

        // Verify structure
        assert!(restored_fs.stat("/test").is_ok());
        assert!(restored_fs.stat("/test/nested/dir").is_ok());
        assert!(restored_fs.stat("/test/file.txt").is_ok());

        // Verify file content
        let fd = restored_fs
            .open_path_with_flags("/test/file.txt", crate::types::O_RDONLY)
            .unwrap();
        let mut buf = [0u8; 20];
        let n = restored_fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello, World!");
    }

    #[test]
    fn test_snapshot_json_format() {
        let mut fs = Fs::new();
        fs.mkdir("/data").unwrap();
        let fd = fs.open_path("/data/test.txt").unwrap();
        fs.write(fd, b"test content").unwrap();
        fs.close(fd).unwrap();

        let snapshot = fs.to_snapshot();
        let json = serde_json::to_string_pretty(&snapshot).unwrap();

        // Verify JSON is readable
        assert!(json.contains("\"next_inode\""));
        assert!(json.contains("\"inodes\""));
    }
}
