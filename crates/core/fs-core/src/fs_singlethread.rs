//! Single-threaded filesystem implementation using Rc<RefCell>
//!
//! This module provides a non-thread-safe filesystem that uses Rc<RefCell>
//! instead of Arc<RwLock>. It's suitable for:
//! - vfs-adapter (runs inside WASM boundary)
//! - vfs-rpc-server (runs inside WASM boundary)
//!
//! For thread-safe usage (e.g., vfs-host), use the thread-safe module instead.

#[cfg(feature = "std")]
use std::cell::RefCell;
#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::rc::Rc;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;
#[cfg(not(feature = "std"))]
use alloc::rc::Rc;
#[cfg(not(feature = "std"))]
use core::cell::RefCell;

use crate::error::FsError;
use crate::handle::FileHandle;
use crate::inode::{FileContent, Inode, Metadata};
use crate::time::{MonotonicCounter, TimeProvider};
use crate::types::*;

// Import logging macros
#[cfg(feature = "logging")]
use crate::{debug, error, trace};

/// Type alias for the internal map type
#[cfg(feature = "std")]
type Map<K, V> = HashMap<K, V>;
#[cfg(not(feature = "std"))]
type Map<K, V> = BTreeMap<K, V>;

/// Type alias for inode reference (single-threaded version)
pub type InodeRef = Rc<RefCell<Inode>>;

/// Main filesystem structure (single-threaded, uses Rc<RefCell>)
pub struct Fs<T: TimeProvider = MonotonicCounter> {
    pub(crate) next_inode: InodeId,
    pub(crate) fd_table: Map<Fd, FileHandle>,
    pub(crate) inode_table: Map<InodeId, Rc<RefCell<Inode>>>,
    pub(crate) root_inode: InodeId,
    pub(crate) time_provider: T,
}

impl<T: TimeProvider> Fs<T> {
    pub fn with_time_provider(time_provider: T) -> Self {
        let mut fs = Self {
            next_inode: 1,
            fd_table: Map::new(),
            inode_table: Map::new(),
            root_inode: 0,
            time_provider,
        };

        // Create root directory inode with proper timestamps
        let timestamp = fs.time_provider.now();
        let mut root_inode = Inode::new_dir(0);
        root_inode.metadata.created = timestamp;
        root_inode.metadata.modified = timestamp;

        let root = Rc::new(RefCell::new(root_inode));
        fs.inode_table.insert(0, root);
        fs.root_inode = 0;

        fs
    }

    fn allocate_inode(&mut self) -> InodeId {
        let id = self.next_inode;
        self.next_inode += 1;
        id
    }

    fn allocate_fd(&mut self) -> Fd {
        // Find the lowest available file descriptor starting from 3
        // This implements POSIX-compliant FD reuse
        let mut fd = 3;
        while self.fd_table.contains_key(&fd) {
            fd += 1;
        }
        fd
    }

    fn find_inode(
        &self,
        parent_inode: &Rc<RefCell<Inode>>,
        name: &str,
    ) -> Result<Rc<RefCell<Inode>>, FsError> {
        let parent = parent_inode.borrow();

        match &parent.content {
            FileContent::Dir(entries) => {
                if let Some(&inode_id) = entries.get(name) {
                    if let Some(inode) = self.inode_table.get(&inode_id) {
                        Ok(inode.clone())
                    } else {
                        Err(FsError::NotFound)
                    }
                } else {
                    Err(FsError::NotFound)
                }
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    fn create_inode(
        &mut self,
        parent_inode: &Rc<RefCell<Inode>>,
        name: &str,
        is_dir: bool,
    ) -> Result<Rc<RefCell<Inode>>, FsError> {
        let mut parent = parent_inode.borrow_mut();

        match &mut parent.content {
            FileContent::Dir(entries) => {
                // Check if already exists
                if entries.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }

                // Create new inode
                let new_inode_id = self.allocate_inode();
                let timestamp = self.time_provider.now();
                let mut new_inode = if is_dir {
                    Inode::new_dir(new_inode_id)
                } else {
                    Inode::new_file(new_inode_id)
                };

                new_inode.metadata.created = timestamp;
                new_inode.metadata.modified = timestamp;

                let new_inode_rc = Rc::new(RefCell::new(new_inode));
                entries.insert(name.into(), new_inode_id);
                self.inode_table.insert(new_inode_id, new_inode_rc.clone());

                // Update parent directory's modification time
                parent.metadata.modified = timestamp;

                Ok(new_inode_rc)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn open_path(&mut self, path: &str) -> Result<Fd, FsError> {
        self.open_path_with_flags(path, O_RDWR | O_CREAT)
    }

    pub fn open_at(&mut self, dir_fd: Fd, path: &str, flags: u32) -> Result<Fd, FsError> {
        debug!(
            "open_at: dir_fd={}, path={}, flags={:#x}",
            dir_fd, path, flags
        );

        // Reject absolute paths. open_at only accepts relative paths
        if path.starts_with('/') {
            error!("open_at: absolute path not allowed");
            return Err(FsError::InvalidArgument);
        }

        // Get the directory file descriptor
        #[allow(clippy::unnecessary_lazy_evaluations)]
        let dir_handle = self.fd_table.get(&dir_fd).ok_or_else(|| {
            error!("open_at: bad directory file descriptor {}", dir_fd);
            FsError::BadFileDescriptor
        })?;

        // Get the directory inode
        let dir_inode = self
            .inode_table
            .get(&dir_handle.inode_id)
            .ok_or(FsError::NotFound)?
            .clone();

        // Verify it's a directory
        {
            let inode = dir_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                error!("open_at: dir_fd {} is not a directory", dir_fd);
                return Err(FsError::NotADirectory);
            }
        }

        // Handle empty path: refers to the directory itself
        if path.is_empty() {
            error!("open_at: empty path not supported yet");
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        // Start from the directory fd, not root
        let mut current_inode = dir_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Handle final component (file name)
        let filename = comps[comps.len() - 1];
        let file_inode = match self.find_inode(&current_inode, filename) {
            Ok(inode) => inode,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                // Create new file if O_CREAT is set
                self.create_inode(&current_inode, filename, false)?
            }
            Err(e) => return Err(e),
        };

        // Check if target is a directory
        {
            let inode = file_inode.borrow();
            if matches!(inode.content, FileContent::Dir(_)) {
                let access_mode = flags & 0x3;
                // POSIX: opening directory with write access is not allowed
                if access_mode == O_WRONLY || access_mode == O_RDWR {
                    return Err(FsError::IsADirectory);
                }
                // O_TRUNC on directory is also not allowed
                if flags & O_TRUNC != 0 {
                    return Err(FsError::IsADirectory);
                }
            }
        }

        // Handle O_TRUNC: truncate file to 0 bytes if flag is set
        if flags & O_TRUNC != 0 {
            let access_mode = flags & 0x3;
            if access_mode == O_RDONLY {
                return Err(FsError::InvalidArgument);
            }
            let mut inode = file_inode.borrow_mut();
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = file_inode.borrow().id;
        let handle = FileHandle::new(inode_id, 0, flags);

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        debug!("open_at: allocated fd={} for inode={}", fd, inode_id);
        Ok(fd)
    }

    pub fn open_path_with_flags(&mut self, path: &str, flags: u32) -> Result<Fd, FsError> {
        debug!("open_path_with_flags: path={}, flags={:#x}", path, flags);

        if path.is_empty() {
            error!("open_path_with_flags: empty path");
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        // Special case: opening root directory "/"
        if comps.is_empty() {
            debug!("open_path_with_flags: opening root directory");
            let handle = FileHandle::new(self.root_inode, 0, flags);
            let fd = self.allocate_fd();
            self.fd_table.insert(fd, handle);
            debug!("open_path_with_flags: allocated fd={} for root", fd);
            return Ok(fd);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Handle final component (file name)
        let filename = comps[comps.len() - 1];
        let file_inode = match self.find_inode(&current_inode, filename) {
            Ok(inode) => inode,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                // Create new file if O_CREAT is set
                self.create_inode(&current_inode, filename, false)?
            }
            Err(e) => return Err(e),
        };

        // Check if target is a directory
        {
            let inode = file_inode.borrow();
            if matches!(inode.content, FileContent::Dir(_)) {
                let access_mode = flags & 0x3;
                // POSIX: opening directory with write access is not allowed
                if access_mode == O_WRONLY || access_mode == O_RDWR {
                    return Err(FsError::IsADirectory);
                }
                // O_TRUNC on directory is also not allowed
                if flags & O_TRUNC != 0 {
                    return Err(FsError::IsADirectory);
                }
            }
        }

        // Handle O_TRUNC: truncate file to 0 bytes if flag is set
        // POSIX requires write permission for O_TRUNC
        if flags & O_TRUNC != 0 {
            let access_mode = flags & 0x3;
            if access_mode == O_RDONLY {
                return Err(FsError::InvalidArgument);
            }
            let mut inode = file_inode.borrow_mut();
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = file_inode.borrow().id;
        let handle = FileHandle::new(inode_id, 0, flags);

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        debug!(
            "open_path_with_flags: allocated fd={} for inode={}",
            fd, inode_id
        );
        Ok(fd)
    }

    pub fn write(&mut self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        trace!("write: fd={}, len={}", fd, buf.len());

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let handle = self.fd_table.get_mut(&fd).ok_or_else(|| {
            error!("write: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        // Check write permission
        let access_mode = handle.flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        let mut inode_ref = inode.borrow_mut();

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                // Handle O_APPEND: move position to end of file before writing
                let pos = if handle.flags & O_APPEND != 0 {
                    let file_size = storage.size();
                    handle.set_position(file_size as u64);
                    file_size
                } else {
                    handle.get_position() as usize
                };

                let n = storage.write(pos, buf);
                handle.set_position(handle.get_position() + n as u64);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();
                debug!("write: fd={}, wrote {} bytes at pos {}", fd, n, pos);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("write: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    /// Append data to file atomically (always writes at end of file)
    pub fn append_write(&mut self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        trace!("append_write: fd={}, len={}", fd, buf.len());

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let handle = self.fd_table.get_mut(&fd).ok_or_else(|| {
            error!("append_write: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        // Check write permission
        let access_mode = handle.flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        let mut inode_ref = inode.borrow_mut();

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                // Always write at the end of file (atomic append)
                let pos = storage.size();
                let n = storage.write(pos, buf);
                handle.set_position((pos + n) as u64);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();
                debug!(
                    "append_write: fd={}, appended {} bytes at pos {}",
                    fd, n, pos
                );
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("append_write: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    pub fn read(&mut self, fd: Fd, out: &mut [u8]) -> Result<usize, FsError> {
        trace!("read: fd={}, buf_len={}", fd, out.len());

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let handle = self.fd_table.get_mut(&fd).ok_or_else(|| {
            error!("read: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        // Check read permission
        let access_mode = handle.flags & 0x3;
        if access_mode == O_WRONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        let inode_ref = inode.borrow();

        match &inode_ref.content {
            FileContent::File(storage) => {
                let pos = handle.get_position() as usize;
                let n = storage.read(pos, out);
                handle.set_position(handle.get_position() + n as u64);
                debug!("read: fd={}, read {} bytes at pos {}", fd, n, pos);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("read: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    pub fn ftruncate(&mut self, fd: Fd, size: u64) -> Result<(), FsError> {
        let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;

        // Check write permission (truncate requires write access)
        let access_mode = handle.flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        let mut inode_ref = inode.borrow_mut();

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                storage.truncate(size as usize);
                inode_ref.metadata.size = size;
                inode_ref.metadata.modified = self.time_provider.now();
                Ok(())
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn close(&mut self, fd: Fd) -> Result<(), FsError> {
        trace!("close: fd={}", fd);

        #[allow(clippy::unnecessary_lazy_evaluations)]
        self.fd_table.remove(&fd).ok_or_else(|| {
            error!("close: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        debug!("close: fd={} closed successfully", fd);
        Ok(())
    }

    pub fn stat(&self, path: &str) -> Result<Metadata, FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            // Root directory
            let root = self
                .inode_table
                .get(&self.root_inode)
                .ok_or(FsError::NotFound)?;
            return Ok(root.borrow().metadata);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate through all components
        for comp in comps.iter() {
            current_inode = self.find_inode(&current_inode, comp)?;
        }

        let metadata = current_inode.borrow().metadata;
        Ok(metadata)
    }

    pub fn fstat(&self, fd: Fd) -> Result<Metadata, FsError> {
        let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        Ok(inode.borrow().metadata)
    }

    pub fn seek(&mut self, fd: Fd, offset: i64, whence: i32) -> Result<u64, FsError> {
        trace!("seek: fd={}, offset={}, whence={}", fd, offset, whence);

        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let handle = self.fd_table.get_mut(&fd).ok_or_else(|| {
            error!("seek: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;
        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        let inode_ref = inode.borrow();

        match &inode_ref.content {
            FileContent::File(storage) => {
                let new_pos = match whence {
                    SEEK_SET => {
                        if offset < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        offset as u64
                    }
                    SEEK_CUR => {
                        let new_pos_signed = handle.get_position() as i64 + offset;
                        if new_pos_signed < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        new_pos_signed as u64
                    }
                    SEEK_END => {
                        let new_pos_signed = storage.size() as i64 + offset;
                        if new_pos_signed < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        new_pos_signed as u64
                    }
                    _ => {
                        error!("seek: invalid whence {}", whence);
                        return Err(FsError::InvalidArgument);
                    }
                };

                handle.set_position(new_pos);
                debug!("seek: fd={}, new_pos={}", fd, new_pos);
                Ok(new_pos)
            }
            FileContent::Dir(_) => {
                error!("seek: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        debug!("mkdir: path={}", path);

        if path.is_empty() {
            error!("mkdir: empty path");
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Create the final directory component
        let dirname = comps[comps.len() - 1];
        self.create_inode(&current_inode, dirname, true)?;
        debug!("mkdir: created directory {}", path);
        Ok(())
    }

    pub fn mkdir_p(&mut self, path: &str) -> Result<(), FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            return Ok(()); // Root already exists
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Create all components as needed
        for comp in comps.iter() {
            current_inode = match self.find_inode(&current_inode, comp) {
                Ok(inode) => {
                    // Verify it's a directory
                    {
                        let borrowed = inode.borrow();
                        if matches!(borrowed.content, FileContent::File(_)) {
                            return Err(FsError::NotADirectory);
                        }
                    }
                    inode
                }
                Err(FsError::NotFound) => {
                    // Create directory
                    self.create_inode(&current_inode, comp, true)?
                }
                Err(e) => return Err(e),
            };
        }

        Ok(())
    }

    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        debug!("unlink: path={}", path);

        if path.is_empty() {
            error!("unlink: empty path");
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            // Cannot unlink root directory
            return Err(FsError::InvalidArgument);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Get the filename to delete
        let filename = comps[comps.len() - 1];

        // Check if the file exists and get its inode
        let target_inode = self.find_inode(&current_inode, filename)?;

        // Check if it's a directory: use IsADirectory error
        {
            let target = target_inode.borrow();
            if matches!(target.content, FileContent::Dir(_)) {
                return Err(FsError::IsADirectory);
            }
        }

        // Remove from parent directory
        let mut parent = current_inode.borrow_mut();
        match &mut parent.content {
            FileContent::Dir(entries) => {
                entries.remove(filename);

                // Update parent directory's modification time
                let timestamp = self.time_provider.now();
                parent.metadata.modified = timestamp;

                debug!("unlink: removed {}", path);
                Ok(())
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
        // Note: The inode will be automatically cleaned up when the Rc ref count reaches 0
    }

    pub fn readdir(&self, path: &str) -> Result<alloc::vec::Vec<alloc::string::String>, FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate through all components
        for comp in comps.iter() {
            current_inode = self.find_inode(&current_inode, comp)?;
        }

        // Check if it's a directory
        let inode = current_inode.borrow();
        match &inode.content {
            FileContent::Dir(entries) => {
                let mut result = alloc::vec::Vec::new();
                for name in entries.keys() {
                    result.push(name.clone());
                }
                Ok(result)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    /// Read directory entries from an open file descriptor
    /// Returns a list of (name, is_dir) tuples
    pub fn readdir_fd(
        &self,
        fd: Fd,
    ) -> Result<alloc::vec::Vec<(alloc::string::String, bool)>, FsError> {
        // Get the file handle
        let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;

        // Get the inode
        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;

        // Check if it's a directory
        let inode_ref = inode.borrow();
        match &inode_ref.content {
            FileContent::Dir(entries) => {
                let mut result = alloc::vec::Vec::new();
                for (name, child_inode_id) in entries.iter() {
                    // Get the child inode to check if it's a directory
                    if let Some(child_inode) = self.inode_table.get(child_inode_id) {
                        let is_dir = child_inode.borrow().metadata.is_dir;
                        result.push((name.clone(), is_dir));
                    }
                }
                Ok(result)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    /// Remove an empty directory
    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        debug!("rmdir: path={}", path);

        if path.is_empty() {
            error!("rmdir: empty path");
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            // Cannot remove root directory
            return Err(FsError::InvalidArgument);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Get the directory name to delete
        let dirname = comps[comps.len() - 1];

        // Check if the directory exists and get its inode
        let target_inode = self.find_inode(&current_inode, dirname)?;

        // Check if it's a directory
        {
            let target = target_inode.borrow();
            match &target.content {
                FileContent::File(_) => {
                    return Err(FsError::NotADirectory);
                }
                FileContent::Dir(entries) => {
                    // Check if directory is empty
                    if !entries.is_empty() {
                        error!("rmdir: directory not empty");
                        return Err(FsError::NotEmpty);
                    }
                }
            }
        }

        // Remove from parent directory
        let mut parent = current_inode.borrow_mut();
        match &mut parent.content {
            FileContent::Dir(entries) => {
                entries.remove(dirname);

                // Update parent directory's modification time
                let timestamp = self.time_provider.now();
                parent.metadata.modified = timestamp;

                debug!("rmdir: removed {}", path);
                Ok(())
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
        // Note: The inode will be automatically cleaned up when the Rc ref count reaches 0
    }

    /// Rename or move a file or directory
    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        debug!("rename: old_path={}, new_path={}", old_path, new_path);

        if old_path.is_empty() || new_path.is_empty() {
            error!("rename: empty path");
            return Err(FsError::InvalidArgument);
        }

        let old_comps: alloc::vec::Vec<&str> = old_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let new_comps: alloc::vec::Vec<&str> = new_path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if old_comps.is_empty() || new_comps.is_empty() {
            // Cannot rename root directory
            return Err(FsError::InvalidArgument);
        }

        // Same path is a no-op
        if old_comps == new_comps {
            return Ok(());
        }

        // Navigate to old parent directory
        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut old_parent = root_inode.clone();

        for comp in old_comps.iter().take(old_comps.len() - 1) {
            old_parent = self.find_inode(&old_parent, comp)?;
            let inode = old_parent.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let old_name = old_comps[old_comps.len() - 1];

        // Verify the source exists and get its type
        let source_inode = self.find_inode(&old_parent, old_name)?;
        let source_is_dir = {
            let inode = source_inode.borrow();
            inode.metadata.is_dir
        };

        // Navigate to new parent directory
        let mut new_parent = root_inode;

        for comp in new_comps.iter().take(new_comps.len() - 1) {
            new_parent = self.find_inode(&new_parent, comp)?;
            let inode = new_parent.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let new_name = new_comps[new_comps.len() - 1];

        // Check if destination exists and validate type compatibility
        if let Ok(dest_inode) = self.find_inode(&new_parent, new_name) {
            let dest = dest_inode.borrow();
            let dest_is_dir = dest.metadata.is_dir;

            if source_is_dir && !dest_is_dir {
                return Err(FsError::NotADirectory);
            }
            if !source_is_dir && dest_is_dir {
                return Err(FsError::IsADirectory);
            }
            if dest_is_dir
                && matches!(&dest.content, FileContent::Dir(entries) if !entries.is_empty())
            {
                return Err(FsError::NotEmpty);
            }
        }

        // Determine if same directory by comparing inode IDs
        let old_parent_id = old_parent.borrow().id;
        let new_parent_id = new_parent.borrow().id;

        let timestamp = self.time_provider.now();

        if old_parent_id == new_parent_id {
            // Same directory: single borrow_mut to avoid double-borrow panic
            let mut parent = old_parent.borrow_mut();
            if let FileContent::Dir(entries) = &mut parent.content {
                if let Some(inode_id) = entries.remove(old_name) {
                    entries.insert(alloc::string::String::from(new_name), inode_id);
                    parent.metadata.modified = timestamp;
                } else {
                    return Err(FsError::NotFound);
                }
            } else {
                return Err(FsError::NotADirectory);
            }
        } else {
            // Cross-directory: remove from old, then insert into new
            let inode_id = {
                let mut parent = old_parent.borrow_mut();
                if let FileContent::Dir(entries) = &mut parent.content {
                    let id = entries.remove(old_name).ok_or(FsError::NotFound)?;
                    parent.metadata.modified = timestamp;
                    id
                } else {
                    return Err(FsError::NotADirectory);
                }
            };

            let mut parent = new_parent.borrow_mut();
            if let FileContent::Dir(entries) = &mut parent.content {
                entries.insert(alloc::string::String::from(new_name), inode_id);
                parent.metadata.modified = timestamp;
            } else {
                return Err(FsError::NotADirectory);
            }
        }

        debug!("rename: {} -> {}", old_path, new_path);
        Ok(())
    }
}

impl Default for Fs<MonotonicCounter> {
    fn default() -> Self {
        Self::new()
    }
}

impl Fs<MonotonicCounter> {
    pub fn new() -> Self {
        Self::with_time_provider(MonotonicCounter::new())
    }
}
