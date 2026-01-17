//! fs-global: Global-lock VFS for benchmarking
//!
//! This crate provides a VFS implementation with a single Mutex for all operations.
//! All operations are serialized through the global lock.
//! Used to benchmark the overhead of global locking vs fine-grained locking.

use std::collections::HashMap;
use std::sync::Mutex;

pub mod error;
pub mod types;

pub use error::FsError;
pub use types::*;

/// File content storage
#[derive(Clone)]
pub enum FileContent {
    File(Vec<u8>),
    Dir(HashMap<String, InodeId>),
}

/// Inode structure
#[derive(Clone)]
pub struct Inode {
    pub id: InodeId,
    pub content: FileContent,
    pub size: u64,
    pub created: u64,
    pub modified: u64,
}

impl Inode {
    fn new_file(id: InodeId) -> Self {
        Self {
            id,
            content: FileContent::File(Vec::new()),
            size: 0,
            created: 0,
            modified: 0,
        }
    }

    fn new_dir(id: InodeId) -> Self {
        Self {
            id,
            content: FileContent::Dir(HashMap::new()),
            size: 0,
            created: 0,
            modified: 0,
        }
    }
}

/// File handle
#[derive(Clone)]
pub struct FileHandle {
    pub inode_id: InodeId,
    pub position: u64,
    pub flags: u32,
}

/// Metadata for stat operations
#[derive(Clone, Debug)]
pub struct Metadata {
    pub size: u64,
    pub is_dir: bool,
    pub created: u64,
    pub modified: u64,
}

/// Inner state protected by Mutex
struct FsInner {
    next_inode: u64,
    next_fd: u32,
    fd_table: HashMap<Fd, FileHandle>,
    inode_table: HashMap<InodeId, Inode>,
    root_inode: InodeId,
}

/// Global-lock filesystem
/// All operations acquire the single Mutex, serializing access.
pub struct Fs {
    inner: Mutex<FsInner>,
}

// Open flags
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;

// Seek whence
pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

impl Fs {
    pub fn new() -> Self {
        let mut inode_table = HashMap::new();
        let root = Inode::new_dir(0);
        inode_table.insert(0, root);

        Self {
            inner: Mutex::new(FsInner {
                next_inode: 1,
                next_fd: 3,
                fd_table: HashMap::new(),
                inode_table,
                root_inode: 0,
            }),
        }
    }

    pub fn mkdir(&self, path: &str) -> Result<(), FsError> {
        let mut inner = self.inner.lock().unwrap();

        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode_id = inner.root_inode;

        // Navigate to parent
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        // Create directory
        let dirname = comps[comps.len() - 1];
        let new_inode_id = inner.next_inode;
        inner.next_inode += 1;

        let new_inode = Inode::new_dir(new_inode_id);
        inner.inode_table.insert(new_inode_id, new_inode);

        let parent = inner.inode_table.get_mut(&current_inode_id).ok_or(FsError::NotFound)?;
        match &mut parent.content {
            FileContent::Dir(entries) => {
                if entries.contains_key(dirname) {
                    return Err(FsError::AlreadyExists);
                }
                entries.insert(dirname.to_string(), new_inode_id);
            }
            FileContent::File(_) => return Err(FsError::NotADirectory),
        }

        Ok(())
    }

    pub fn open_path(&self, path: &str) -> Result<Fd, FsError> {
        self.open_path_with_flags(path, O_RDWR | O_CREAT)
    }

    pub fn open_path_with_flags(&self, path: &str, flags: u32) -> Result<Fd, FsError> {
        let mut inner = self.inner.lock().unwrap();

        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        if comps.is_empty() {
            // Opening root directory
            let fd = inner.next_fd;
            let root_inode = inner.root_inode;
            inner.next_fd += 1;
            inner.fd_table.insert(fd, FileHandle {
                inode_id: root_inode,
                position: 0,
                flags,
            });
            return Ok(fd);
        }

        let mut current_inode_id = inner.root_inode;

        // Navigate to parent
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        // Find or create file
        let filename = comps[comps.len() - 1];
        let parent = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;

        let file_inode_id = match &parent.content {
            FileContent::Dir(entries) => {
                if let Some(&id) = entries.get(filename) {
                    id
                } else if flags & O_CREAT != 0 {
                    // Create new file
                    let new_id = inner.next_inode;
                    inner.next_inode += 1;
                    let new_inode = Inode::new_file(new_id);
                    inner.inode_table.insert(new_id, new_inode);

                    // Add to parent
                    let parent = inner.inode_table.get_mut(&current_inode_id).unwrap();
                    if let FileContent::Dir(entries) = &mut parent.content {
                        entries.insert(filename.to_string(), new_id);
                    }
                    new_id
                } else {
                    return Err(FsError::NotFound);
                }
            }
            FileContent::File(_) => return Err(FsError::NotADirectory),
        };

        // Handle O_TRUNC
        if flags & O_TRUNC != 0 {
            if let Some(inode) = inner.inode_table.get_mut(&file_inode_id) {
                if let FileContent::File(data) = &mut inode.content {
                    data.clear();
                    inode.size = 0;
                }
            }
        }

        let fd = inner.next_fd;
        inner.next_fd += 1;
        inner.fd_table.insert(fd, FileHandle {
            inode_id: file_inode_id,
            position: 0,
            flags,
        });

        Ok(fd)
    }

    pub fn read(&self, fd: Fd, buf: &mut [u8]) -> Result<usize, FsError> {
        let mut inner = self.inner.lock().unwrap();

        let handle = inner.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?.clone();
        let inode = inner.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;

        match &inode.content {
            FileContent::File(data) => {
                let pos = handle.position as usize;
                if pos >= data.len() {
                    return Ok(0);
                }
                let available = data.len() - pos;
                let to_read = buf.len().min(available);
                buf[..to_read].copy_from_slice(&data[pos..pos + to_read]);

                // Update position
                if let Some(h) = inner.fd_table.get_mut(&fd) {
                    h.position += to_read as u64;
                }

                Ok(to_read)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        let mut inner = self.inner.lock().unwrap();

        let handle = inner.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?.clone();
        let inode = inner.inode_table.get_mut(&handle.inode_id).ok_or(FsError::NotFound)?;

        match &mut inode.content {
            FileContent::File(data) => {
                let pos = if handle.flags & O_APPEND != 0 {
                    data.len()
                } else {
                    handle.position as usize
                };

                // Extend if needed
                if pos + buf.len() > data.len() {
                    data.resize(pos + buf.len(), 0);
                }
                data[pos..pos + buf.len()].copy_from_slice(buf);
                inode.size = data.len() as u64;

                // Update position
                if let Some(h) = inner.fd_table.get_mut(&fd) {
                    h.position = (pos + buf.len()) as u64;
                }

                Ok(buf.len())
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn append_write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        let mut inner = self.inner.lock().unwrap();

        let handle = inner.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?.clone();

        // Get new position after write
        let new_position = {
            let inode = inner.inode_table.get_mut(&handle.inode_id).ok_or(FsError::NotFound)?;
            match &mut inode.content {
                FileContent::File(data) => {
                    data.extend_from_slice(buf);
                    inode.size = data.len() as u64;
                    data.len() as u64
                }
                FileContent::Dir(_) => return Err(FsError::BadFileDescriptor),
            }
        };

        // Update position
        if let Some(h) = inner.fd_table.get_mut(&fd) {
            h.position = new_position;
        }

        Ok(buf.len())
    }

    pub fn seek(&self, fd: Fd, offset: i64, whence: u32) -> Result<u64, FsError> {
        let mut inner = self.inner.lock().unwrap();

        let handle = inner.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = inner.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;

        let size = match &inode.content {
            FileContent::File(data) => data.len() as i64,
            FileContent::Dir(_) => return Err(FsError::BadFileDescriptor),
        };

        let new_pos = match whence {
            SEEK_SET => offset,
            SEEK_CUR => handle.position as i64 + offset,
            SEEK_END => size + offset,
            _ => return Err(FsError::InvalidArgument),
        };

        if new_pos < 0 {
            return Err(FsError::InvalidArgument);
        }

        if let Some(h) = inner.fd_table.get_mut(&fd) {
            h.position = new_pos as u64;
        }

        Ok(new_pos as u64)
    }

    pub fn close(&self, fd: Fd) -> Result<(), FsError> {
        let mut inner = self.inner.lock().unwrap();
        inner.fd_table.remove(&fd).ok_or(FsError::BadFileDescriptor)?;
        Ok(())
    }

    pub fn fstat(&self, fd: Fd) -> Result<Metadata, FsError> {
        let inner = self.inner.lock().unwrap();

        let handle = inner.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = inner.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;

        Ok(Metadata {
            size: inode.size,
            is_dir: matches!(inode.content, FileContent::Dir(_)),
            created: inode.created,
            modified: inode.modified,
        })
    }

    pub fn stat(&self, path: &str) -> Result<Metadata, FsError> {
        let inner = self.inner.lock().unwrap();

        let comps: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();

        let mut current_inode_id = inner.root_inode;
        for comp in &comps {
            let inode = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        let inode = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
        Ok(Metadata {
            size: inode.size,
            is_dir: matches!(inode.content, FileContent::Dir(_)),
            created: inode.created,
            modified: inode.modified,
        })
    }

    pub fn readdir(&self, fd: Fd) -> Result<Vec<(String, bool)>, FsError> {
        self.readdir_fd(fd)
    }

    pub fn readdir_fd(&self, fd: Fd) -> Result<Vec<(String, bool)>, FsError> {
        let inner = self.inner.lock().unwrap();

        let handle = inner.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = inner.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;

        match &inode.content {
            FileContent::Dir(entries) => {
                let result: Vec<(String, bool)> = entries.iter().map(|(name, id)| {
                    let is_dir = inner.inode_table.get(id)
                        .map(|i| matches!(i.content, FileContent::Dir(_)))
                        .unwrap_or(false);
                    (name.clone(), is_dir)
                }).collect();
                Ok(result)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn ftruncate(&self, fd: Fd, size: u64) -> Result<(), FsError> {
        let mut inner = self.inner.lock().unwrap();

        let inode_id = inner.fd_table.get(&fd)
            .ok_or(FsError::BadFileDescriptor)?
            .inode_id;
        let inode = inner.inode_table.get_mut(&inode_id).ok_or(FsError::NotFound)?;

        match &mut inode.content {
            FileContent::File(data) => {
                data.resize(size as usize, 0);
                inode.size = size;
                Ok(())
            }
            FileContent::Dir(_) => Err(FsError::IsADirectory),
        }
    }

    pub fn unlink(&self, path: &str) -> Result<(), FsError> {
        let mut inner = self.inner.lock().unwrap();

        let comps: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode_id = inner.root_inode;
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        let filename = comps[comps.len() - 1];
        let parent = inner.inode_table.get_mut(&current_inode_id).ok_or(FsError::NotFound)?;

        match &mut parent.content {
            FileContent::Dir(entries) => {
                let file_id = entries.remove(filename).ok_or(FsError::NotFound)?;
                inner.inode_table.remove(&file_id);
                Ok(())
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn rmdir(&self, path: &str) -> Result<(), FsError> {
        let mut inner = self.inner.lock().unwrap();

        let comps: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode_id = inner.root_inode;
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        let dirname = comps[comps.len() - 1];

        // Check if directory is empty
        let dir_id = {
            let parent = inner.inode_table.get(&current_inode_id).ok_or(FsError::NotFound)?;
            match &parent.content {
                FileContent::Dir(entries) => *entries.get(dirname).ok_or(FsError::NotFound)?,
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        };

        let is_empty = {
            let dir = inner.inode_table.get(&dir_id).ok_or(FsError::NotFound)?;
            match &dir.content {
                FileContent::Dir(entries) => entries.is_empty(),
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        };

        if !is_empty {
            return Err(FsError::NotEmpty);
        }

        // Remove from parent
        let parent = inner.inode_table.get_mut(&current_inode_id).ok_or(FsError::NotFound)?;
        if let FileContent::Dir(entries) = &mut parent.content {
            entries.remove(dirname);
        }
        inner.inode_table.remove(&dir_id);

        Ok(())
    }
}

impl Default for Fs {
    fn default() -> Self {
        Self::new()
    }
}

// Ensure Fs is Send + Sync
unsafe impl Send for Fs {}
unsafe impl Sync for Fs {}
