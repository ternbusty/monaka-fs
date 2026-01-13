//! fs-unsafe: No-lock VFS for benchmarking
//!
//! This crate wraps fs-core and removes all synchronization.
//! It uses the same data structures (BlockStorage, Inode, etc.) as fs-core,
//! but accesses them without locks to measure the overhead of locking.
//!
//! WARNING: This is ONLY for benchmarking. Do not use in production.
//! This WILL cause data races under concurrent access.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// Re-export fs-core types
pub use fs_core::{
    BlockStorage, Fd, FileContent, FsError, Inode, InodeId, Metadata, BLOCK_SIZE, O_APPEND,
    O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
};

/// File handle (simplified version without atomic position for benchmarking)
#[derive(Clone)]
pub struct FileHandle {
    pub inode_id: InodeId,
    pub position: u64,
    pub flags: u32,
}

/// Inner state - no synchronization
struct FsInner {
    fd_table: HashMap<Fd, FileHandle>,
    inode_table: HashMap<InodeId, Inode>,
    root_inode: InodeId,
}

/// No-lock filesystem
/// Uses the same Inode/BlockStorage as fs-core, but without any synchronization.
/// This WILL cause data races under concurrent access - for benchmarking only.
pub struct Fs {
    next_inode: AtomicU64,
    next_fd: AtomicU32,
    inner: UnsafeCell<FsInner>,
}

impl Fs {
    pub fn new() -> Self {
        let mut inode_table = HashMap::new();
        let root = Inode::new_dir(0);
        inode_table.insert(0, root);

        Self {
            next_inode: AtomicU64::new(1),
            next_fd: AtomicU32::new(3),
            inner: UnsafeCell::new(FsInner {
                fd_table: HashMap::new(),
                inode_table,
                root_inode: 0,
            }),
        }
    }

    /// UNSAFE: Get mutable reference to inner state without any synchronization
    #[inline]
    fn inner(&self) -> &mut FsInner {
        unsafe { &mut *self.inner.get() }
    }

    fn allocate_inode(&self) -> InodeId {
        self.next_inode.fetch_add(1, Ordering::Relaxed)
    }

    fn allocate_fd(&self) -> Fd {
        self.next_fd.fetch_add(1, Ordering::Relaxed)
    }

    pub fn mkdir(&self, path: &str) -> Result<(), FsError> {
        let inner = self.inner();

        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode_id = inner.root_inode;

        // Navigate to parent
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner
                .inode_table
                .get(&current_inode_id)
                .ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        // Create directory
        let dirname = comps[comps.len() - 1];
        let new_inode_id = self.allocate_inode();
        let new_inode = Inode::new_dir(new_inode_id);
        inner.inode_table.insert(new_inode_id, new_inode);

        let parent = inner
            .inode_table
            .get_mut(&current_inode_id)
            .ok_or(FsError::NotFound)?;
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
        let inner = self.inner();

        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            // Opening root directory
            let fd = self.allocate_fd();
            inner.fd_table.insert(
                fd,
                FileHandle {
                    inode_id: inner.root_inode,
                    position: 0,
                    flags,
                },
            );
            return Ok(fd);
        }

        let mut current_inode_id = inner.root_inode;

        // Navigate to parent
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner
                .inode_table
                .get(&current_inode_id)
                .ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        // Find or create file
        let filename = comps[comps.len() - 1];
        let parent = inner
            .inode_table
            .get(&current_inode_id)
            .ok_or(FsError::NotFound)?;

        let file_inode_id = match &parent.content {
            FileContent::Dir(entries) => {
                if let Some(&id) = entries.get(filename) {
                    id
                } else if flags & O_CREAT != 0 {
                    // Create new file
                    let new_id = self.allocate_inode();
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
                if let FileContent::File(storage) = &mut inode.content {
                    storage.truncate(0);
                    inode.metadata.size = 0;
                }
            }
        }

        let fd = self.allocate_fd();
        inner.fd_table.insert(
            fd,
            FileHandle {
                inode_id: file_inode_id,
                position: 0,
                flags,
            },
        );

        Ok(fd)
    }

    pub fn read(&self, fd: Fd, buf: &mut [u8]) -> Result<usize, FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?
            .clone();
        let inode = inner
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        match &inode.content {
            FileContent::File(storage) => {
                let pos = handle.position as usize;
                let n = storage.read(pos, buf);

                // Update position
                if let Some(h) = inner.fd_table.get_mut(&fd) {
                    h.position += n as u64;
                }

                Ok(n)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?
            .clone();
        let inode = inner
            .inode_table
            .get_mut(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        match &mut inode.content {
            FileContent::File(storage) => {
                let pos = if handle.flags & O_APPEND != 0 {
                    storage.size()
                } else {
                    handle.position as usize
                };

                let n = storage.write(pos, buf);
                inode.metadata.size = storage.size() as u64;

                // Update position
                if let Some(h) = inner.fd_table.get_mut(&fd) {
                    h.position = (pos + n) as u64;
                }

                Ok(n)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn append_write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?
            .clone();
        let inode = inner
            .inode_table
            .get_mut(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        match &mut inode.content {
            FileContent::File(storage) => {
                let pos = storage.size();
                let n = storage.write(pos, buf);
                inode.metadata.size = storage.size() as u64;

                // Update position
                if let Some(h) = inner.fd_table.get_mut(&fd) {
                    h.position = (pos + n) as u64;
                }

                Ok(n)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn seek(&self, fd: Fd, offset: i64, whence: u32) -> Result<u64, FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?;
        let inode = inner
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        let size = match &inode.content {
            FileContent::File(storage) => storage.size() as i64,
            FileContent::Dir(_) => return Err(FsError::BadFileDescriptor),
        };

        const SEEK_SET: u32 = 0;
        const SEEK_CUR: u32 = 1;
        const SEEK_END: u32 = 2;

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
        let inner = self.inner();
        inner
            .fd_table
            .remove(&fd)
            .ok_or(FsError::BadFileDescriptor)?;
        Ok(())
    }

    pub fn fstat(&self, fd: Fd) -> Result<Metadata, FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?;
        let inode = inner
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        Ok(inode.metadata)
    }

    pub fn stat(&self, path: &str) -> Result<Metadata, FsError> {
        let inner = self.inner();

        let comps: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut current_inode_id = inner.root_inode;
        for comp in &comps {
            let inode = inner
                .inode_table
                .get(&current_inode_id)
                .ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        let inode = inner
            .inode_table
            .get(&current_inode_id)
            .ok_or(FsError::NotFound)?;
        Ok(inode.metadata)
    }

    pub fn readdir(&self, fd: Fd) -> Result<Vec<(String, bool)>, FsError> {
        self.readdir_fd(fd)
    }

    pub fn readdir_fd(&self, fd: Fd) -> Result<Vec<(String, bool)>, FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?;
        let inode = inner
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        match &inode.content {
            FileContent::Dir(entries) => {
                let result: Vec<(String, bool)> = entries
                    .iter()
                    .map(|(name, id)| {
                        let is_dir = inner
                            .inode_table
                            .get(id)
                            .map(|i| i.metadata.is_dir)
                            .unwrap_or(false);
                        (name.clone(), is_dir)
                    })
                    .collect();
                Ok(result)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn ftruncate(&self, fd: Fd, size: u64) -> Result<(), FsError> {
        let inner = self.inner();

        let handle = inner
            .fd_table
            .get(&fd)
            .ok_or(FsError::BadFileDescriptor)?;
        let inode = inner
            .inode_table
            .get_mut(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        match &mut inode.content {
            FileContent::File(storage) => {
                storage.truncate(size as usize);
                inode.metadata.size = size;
                Ok(())
            }
            FileContent::Dir(_) => Err(FsError::IsADirectory),
        }
    }

    pub fn unlink(&self, path: &str) -> Result<(), FsError> {
        let inner = self.inner();

        let comps: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode_id = inner.root_inode;
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner
                .inode_table
                .get(&current_inode_id)
                .ok_or(FsError::NotFound)?;
            match &inode.content {
                FileContent::Dir(entries) => {
                    current_inode_id = *entries.get(*comp).ok_or(FsError::NotFound)?;
                }
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        }

        let filename = comps[comps.len() - 1];
        let parent = inner
            .inode_table
            .get_mut(&current_inode_id)
            .ok_or(FsError::NotFound)?;

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
        let inner = self.inner();

        let comps: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode_id = inner.root_inode;
        for comp in comps.iter().take(comps.len() - 1) {
            let inode = inner
                .inode_table
                .get(&current_inode_id)
                .ok_or(FsError::NotFound)?;
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
            let parent = inner
                .inode_table
                .get(&current_inode_id)
                .ok_or(FsError::NotFound)?;
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
        let parent = inner
            .inode_table
            .get_mut(&current_inode_id)
            .ok_or(FsError::NotFound)?;
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

// UNSAFE: Intentionally allow Send + Sync without synchronization
// This WILL cause data races - for benchmarking only
unsafe impl Send for Fs {}
unsafe impl Sync for Fs {}
