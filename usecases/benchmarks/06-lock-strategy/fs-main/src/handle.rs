use crate::types::InodeId;

/// File handle representing an open file descriptor
pub struct FileHandle {
    pub inode_id: InodeId,
    pub position: u64,
    pub flags: u32,
}
