#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;

// Thread-safe inode reference type
// Uses Arc<RwLock<>> with std feature (default) for concurrent read access
// Falls back to Rc<RefCell<>> for no_std environments
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::{Arc, RwLock};

#[cfg(feature = "std")]
use dashmap::DashMap;

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

/// Macro for read access to inode (uses RwLock::read for std, RefCell::borrow for no_std)
macro_rules! inode_read {
    ($inode:expr) => {{
        #[cfg(feature = "std")]
        {
            $inode.read().unwrap()
        }
        #[cfg(not(feature = "std"))]
        {
            $inode.borrow()
        }
    }};
}

/// Macro for write access to inode (uses RwLock::write for std, RefCell::borrow_mut for no_std)
macro_rules! inode_write {
    ($inode:expr) => {{
        #[cfg(feature = "std")]
        {
            $inode.write().unwrap()
        }
        #[cfg(not(feature = "std"))]
        {
            $inode.borrow_mut()
        }
    }};
}

/// Type alias for inode reference - thread-safe with std, single-threaded without
#[cfg(feature = "std")]
pub type InodeRef = Arc<RwLock<Inode>>;
#[cfg(not(feature = "std"))]
pub type InodeRef = Rc<RefCell<Inode>>;

/// Type aliases for concurrent collections (std) vs single-threaded (no_std)
#[cfg(feature = "std")]
pub type FdTable = DashMap<Fd, FileHandle>;
#[cfg(not(feature = "std"))]
pub type FdTable = BTreeMap<Fd, FileHandle>;

#[cfg(feature = "std")]
pub type InodeTable = DashMap<InodeId, InodeRef>;
#[cfg(not(feature = "std"))]
pub type InodeTable = BTreeMap<InodeId, InodeRef>;

/// Main filesystem structure
/// With std feature: uses DashMap for lock-free concurrent access
/// Without std: uses BTreeMap for single-threaded environments
#[cfg(feature = "std")]
pub struct Fs<T: TimeProvider = MonotonicCounter> {
    pub(crate) next_inode: AtomicU64,
    pub(crate) next_fd: AtomicU32,
    pub(crate) fd_table: FdTable,
    pub(crate) inode_table: InodeTable,
    pub(crate) root_inode: InodeId,
    pub(crate) time_provider: T,
}

#[cfg(not(feature = "std"))]
pub struct Fs<T: TimeProvider = MonotonicCounter> {
    pub(crate) next_inode: InodeId,
    pub(crate) fd_table: FdTable,
    pub(crate) inode_table: InodeTable,
    pub(crate) root_inode: InodeId,
    pub(crate) time_provider: T,
}

impl<T: TimeProvider> Fs<T> {
    /// Create a new InodeRef (thread-safe with std, single-threaded without)
    #[cfg(feature = "std")]
    fn new_inode_ref(inode: Inode) -> InodeRef {
        Arc::new(RwLock::new(inode))
    }

    #[cfg(not(feature = "std"))]
    fn new_inode_ref(inode: Inode) -> InodeRef {
        Rc::new(RefCell::new(inode))
    }

    #[cfg(feature = "std")]
    pub fn with_time_provider(time_provider: T) -> Self {
        // Create root directory inode with proper timestamps
        let timestamp = time_provider.now();
        let mut root_inode = Inode::new_dir(0);
        root_inode.metadata.created = timestamp;
        root_inode.metadata.modified = timestamp;

        let inode_table = DashMap::new();
        inode_table.insert(0, Self::new_inode_ref(root_inode));

        Self {
            next_inode: AtomicU64::new(1),
            next_fd: AtomicU32::new(3), // Start from 3 (0,1,2 reserved for stdin/out/err)
            fd_table: DashMap::new(),
            inode_table,
            root_inode: 0,
            time_provider,
        }
    }

    #[cfg(not(feature = "std"))]
    pub fn with_time_provider(time_provider: T) -> Self {
        let mut fs = Self {
            next_inode: 1,
            fd_table: BTreeMap::new(),
            inode_table: BTreeMap::new(),
            root_inode: 0,
            time_provider,
        };

        // Create root directory inode with proper timestamps
        let timestamp = fs.time_provider.now();
        let mut root_inode = Inode::new_dir(0);
        root_inode.metadata.created = timestamp;
        root_inode.metadata.modified = timestamp;

        let root = Self::new_inode_ref(root_inode);
        fs.inode_table.insert(0, root);
        fs.root_inode = 0;

        fs
    }

    #[cfg(feature = "std")]
    fn allocate_inode(&self) -> InodeId {
        self.next_inode.fetch_add(1, Ordering::Relaxed) as InodeId
    }

    #[cfg(not(feature = "std"))]
    fn allocate_inode(&mut self) -> InodeId {
        let id = self.next_inode;
        self.next_inode += 1;
        id
    }

    #[cfg(feature = "std")]
    fn allocate_fd(&self) -> Fd {
        // Use atomic counter for thread-safe fd allocation
        self.next_fd.fetch_add(1, Ordering::Relaxed)
    }

    #[cfg(not(feature = "std"))]
    fn allocate_fd(&mut self) -> Fd {
        // Find the lowest available file descriptor starting from 3
        // This implements POSIX-compliant FD reuse
        let mut fd = 3;
        while self.fd_table.contains_key(&fd) {
            fd += 1;
        }
        fd
    }

    fn find_inode(&self, parent_inode: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
        let parent = inode_read!(parent_inode);

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

    #[cfg(feature = "std")]
    fn create_inode(
        &self,
        parent_inode: &InodeRef,
        name: &str,
        is_dir: bool,
    ) -> Result<InodeRef, FsError> {
        let mut parent = inode_write!(parent_inode);

        match &mut parent.content {
            FileContent::Dir(entries) => {
                if entries.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }

                let new_inode_id = self.allocate_inode();
                let timestamp = self.time_provider.now();
                let mut new_inode = if is_dir {
                    Inode::new_dir(new_inode_id)
                } else {
                    Inode::new_file(new_inode_id)
                };

                new_inode.metadata.created = timestamp;
                new_inode.metadata.modified = timestamp;

                let new_inode_ref = Self::new_inode_ref(new_inode);
                entries.insert(name.into(), new_inode_id);
                self.inode_table.insert(new_inode_id, new_inode_ref.clone());
                parent.metadata.modified = timestamp;

                Ok(new_inode_ref)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    #[cfg(not(feature = "std"))]
    fn create_inode(
        &mut self,
        parent_inode: &InodeRef,
        name: &str,
        is_dir: bool,
    ) -> Result<InodeRef, FsError> {
        let mut parent = inode_write!(parent_inode);

        match &mut parent.content {
            FileContent::Dir(entries) => {
                if entries.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }

                let new_inode_id = self.allocate_inode();
                let timestamp = self.time_provider.now();
                let mut new_inode = if is_dir {
                    Inode::new_dir(new_inode_id)
                } else {
                    Inode::new_file(new_inode_id)
                };

                new_inode.metadata.created = timestamp;
                new_inode.metadata.modified = timestamp;

                let new_inode_ref = Self::new_inode_ref(new_inode);
                entries.insert(name.into(), new_inode_id);
                self.inode_table.insert(new_inode_id, new_inode_ref.clone());
                parent.metadata.modified = timestamp;

                Ok(new_inode_ref)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    #[cfg(feature = "std")]
    pub fn open_path(&self, path: &str) -> Result<Fd, FsError> {
        self.open_path_with_flags(path, O_RDWR | O_CREAT)
    }

    #[cfg(not(feature = "std"))]
    pub fn open_path(&mut self, path: &str) -> Result<Fd, FsError> {
        self.open_path_with_flags(path, O_RDWR | O_CREAT)
    }

    #[cfg(feature = "std")]
    pub fn open_at(&self, dir_fd: Fd, path: &str, flags: u32) -> Result<Fd, FsError> {
        debug!(
            "open_at: dir_fd={}, path={}, flags={:#x}",
            dir_fd, path, flags
        );

        // Reject absolute paths. open_at only accepts relative paths
        if path.starts_with('/') {
            error!("open_at: absolute path not allowed");
            return Err(FsError::InvalidArgument);
        }

        // Get the directory inode_id from fd_table (extract and drop guard early)
        let dir_inode_id = {
            #[allow(clippy::unnecessary_lazy_evaluations)]
            let dir_handle = self.fd_table.get(&dir_fd).ok_or_else(|| {
                error!("open_at: bad directory file descriptor {}", dir_fd);
                FsError::BadFileDescriptor
            })?;
            dir_handle.inode_id
        }; // dir_handle guard dropped here

        // Get the directory inode
        let dir_inode = self
            .inode_table
            .get(&dir_inode_id)
            .ok_or(FsError::NotFound)?
            .clone();

        // Verify it's a directory
        {
            let inode = inode_read!(dir_inode);
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
            let inode = inode_read!(current_inode);
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
            let inode = inode_read!(file_inode);
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
            let mut inode = inode_write!(file_inode);
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = inode_read!(file_inode).id;
        let handle = FileHandle::new(inode_id, 0, flags);

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        debug!("open_at: allocated fd={} for inode={}", fd, inode_id);
        Ok(fd)
    }

    #[cfg(not(feature = "std"))]
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

        // Get the directory inode_id from fd_table (extract and drop guard early)
        let dir_inode_id = {
            #[allow(clippy::unnecessary_lazy_evaluations)]
            let dir_handle = self.fd_table.get(&dir_fd).ok_or_else(|| {
                error!("open_at: bad directory file descriptor {}", dir_fd);
                FsError::BadFileDescriptor
            })?;
            dir_handle.inode_id
        }; // dir_handle guard dropped here

        // Get the directory inode
        let dir_inode = self
            .inode_table
            .get(&dir_inode_id)
            .ok_or(FsError::NotFound)?
            .clone();

        // Verify it's a directory
        {
            let inode = inode_read!(dir_inode);
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
            let inode = inode_read!(current_inode);
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
            let inode = inode_read!(file_inode);
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
            let mut inode = inode_write!(file_inode);
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = inode_read!(file_inode).id;
        let handle = FileHandle::new(inode_id, 0, flags);

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        debug!("open_at: allocated fd={} for inode={}", fd, inode_id);
        Ok(fd)
    }

    #[cfg(feature = "std")]
    pub fn open_path_with_flags(&self, path: &str, flags: u32) -> Result<Fd, FsError> {
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
            let inode = inode_read!(current_inode);
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
            let inode = inode_read!(file_inode);
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
            let mut inode = inode_write!(file_inode);
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = inode_read!(file_inode).id;
        let handle = FileHandle::new(inode_id, 0, flags);

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        debug!(
            "open_path_with_flags: allocated fd={} for inode={}",
            fd, inode_id
        );
        Ok(fd)
    }

    #[cfg(not(feature = "std"))]
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
            let inode = inode_read!(current_inode);
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
            let inode = inode_read!(file_inode);
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
            let mut inode = inode_write!(file_inode);
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = inode_read!(file_inode).id;
        let handle = FileHandle::new(inode_id, 0, flags);

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        debug!(
            "open_path_with_flags: allocated fd={} for inode={}",
            fd, inode_id
        );
        Ok(fd)
    }

    #[cfg(feature = "std")]
    pub fn write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        trace!("write: fd={}, len={}", fd, buf.len());

        // Extract handle data early and drop guard
        let (inode_id, flags, position) = {
            #[allow(clippy::unnecessary_lazy_evaluations)]
            let handle = self.fd_table.get(&fd).ok_or_else(|| {
                error!("write: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;
            (handle.inode_id, handle.flags, handle.get_position())
        };

        // Check write permission
        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                // Handle O_APPEND: move position to end of file before writing
                let pos = if flags & O_APPEND != 0 {
                    storage.size()
                } else {
                    position as usize
                };

                let n = storage.write(pos, buf);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();

                // Update position after write (re-acquire handle)
                drop(inode_ref);
                if let Some(handle) = self.fd_table.get(&fd) {
                    handle.set_position((pos + n) as u64);
                }

                debug!("write: fd={}, wrote {} bytes at pos {}", fd, n, pos);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("write: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    /// Write data at a specific offset atomically (seek + write in one lock acquisition)
    #[cfg(feature = "std")]
    pub fn write_at(&self, fd: Fd, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        trace!("write_at: fd={}, offset={}, len={}", fd, offset, buf.len());

        let (inode_id, flags) = {
            #[allow(clippy::unnecessary_lazy_evaluations)]
            let handle = self.fd_table.get(&fd).ok_or_else(|| {
                error!("write_at: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;
            (handle.inode_id, handle.flags)
        };

        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                let n = storage.write(offset as usize, buf);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();

                drop(inode_ref);
                if let Some(handle) = self.fd_table.get(&fd) {
                    handle.set_position(offset + n as u64);
                }

                debug!(
                    "write_at: fd={}, wrote {} bytes at offset {}",
                    fd, n, offset
                );
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("write_at: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    #[cfg(not(feature = "std"))]
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
        let mut inode_ref = inode_write!(inode);

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
    #[cfg(feature = "std")]
    pub fn append_write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        trace!("append_write: fd={}, len={}", fd, buf.len());

        // Extract handle data early and drop guard
        let (inode_id, flags) = {
            #[allow(clippy::unnecessary_lazy_evaluations)]
            let handle = self.fd_table.get(&fd).ok_or_else(|| {
                error!("append_write: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;
            (handle.inode_id, handle.flags)
        };

        // Check write permission
        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                // Always write at the end of file (atomic append)
                let pos = storage.size();
                let n = storage.write(pos, buf);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();

                // Update position after write (re-acquire handle)
                drop(inode_ref);
                if let Some(handle) = self.fd_table.get(&fd) {
                    handle.set_position((pos + n) as u64);
                }

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

    /// Append data to file atomically (always writes at end of file)
    #[cfg(not(feature = "std"))]
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
        let mut inode_ref = inode_write!(inode);

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

    /// Read data from file - now takes &self since position uses Cell for interior mutability
    pub fn read(&self, fd: Fd, out: &mut [u8]) -> Result<usize, FsError> {
        trace!("read: fd={}, buf_len={}", fd, out.len());

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let handle = self.fd_table.get(&fd).ok_or_else(|| {
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
        let inode_ref = inode_read!(inode);

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

    /// Read data at a specific offset atomically (seek + read in one lock acquisition)
    #[cfg(feature = "std")]
    pub fn read_at(&self, fd: Fd, offset: u64, out: &mut [u8]) -> Result<usize, FsError> {
        trace!(
            "read_at: fd={}, offset={}, buf_len={}",
            fd,
            offset,
            out.len()
        );

        let (inode_id, flags) = {
            #[allow(clippy::unnecessary_lazy_evaluations)]
            let handle = self.fd_table.get(&fd).ok_or_else(|| {
                error!("read_at: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;
            (handle.inode_id, handle.flags)
        };

        let access_mode = flags & 0x3;
        if access_mode == O_WRONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&inode_id).ok_or(FsError::NotFound)?;
        let inode_ref = inode_read!(inode);

        match &inode_ref.content {
            FileContent::File(storage) => {
                let n = storage.read(offset as usize, out);
                drop(inode_ref);
                if let Some(handle) = self.fd_table.get(&fd) {
                    handle.set_position(offset + n as u64);
                }
                debug!("read_at: fd={}, read {} bytes at offset {}", fd, n, offset);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("read_at: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    #[cfg(feature = "std")]
    pub fn ftruncate(&self, fd: Fd, size: u64) -> Result<(), FsError> {
        // Extract handle data early and drop guard
        let (inode_id, flags) = {
            let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
            (handle.inode_id, handle.flags)
        };

        // Check write permission (truncate requires write access)
        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

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

    #[cfg(not(feature = "std"))]
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
        let mut inode_ref = inode_write!(inode);

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

    #[cfg(feature = "std")]
    pub fn close(&self, fd: Fd) -> Result<(), FsError> {
        trace!("close: fd={}", fd);

        #[allow(clippy::unnecessary_lazy_evaluations)]
        self.fd_table.remove(&fd).ok_or_else(|| {
            error!("close: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        debug!("close: fd={} closed successfully", fd);
        Ok(())
    }

    #[cfg(not(feature = "std"))]
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
            return Ok(inode_read!(root).metadata);
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

        Ok(inode_read!(current_inode).metadata)
    }

    pub fn fstat(&self, fd: Fd) -> Result<Metadata, FsError> {
        let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        Ok(inode_read!(inode).metadata)
    }

    /// Seek to a position in file - now takes &self since position uses Cell for interior mutability
    pub fn seek(&self, fd: Fd, offset: i64, whence: i32) -> Result<u64, FsError> {
        trace!("seek: fd={}, offset={}, whence={}", fd, offset, whence);

        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let handle = self.fd_table.get(&fd).ok_or_else(|| {
            error!("seek: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;
        let inode = self
            .inode_table
            .get(&handle.inode_id)
            .ok_or(FsError::NotFound)?;
        let inode_ref = inode_read!(inode);

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

    #[cfg(feature = "std")]
    pub fn mkdir(&self, path: &str) -> Result<(), FsError> {
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
            let inode = inode_read!(current_inode);
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

    #[cfg(not(feature = "std"))]
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
            let inode = inode_read!(current_inode);
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

    #[cfg(feature = "std")]
    pub fn mkdir_p(&self, path: &str) -> Result<(), FsError> {
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
                        let borrowed = inode_read!(inode);
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

    #[cfg(not(feature = "std"))]
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
                        let borrowed = inode_read!(inode);
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

    #[cfg(feature = "std")]
    pub fn unlink(&self, path: &str) -> Result<(), FsError> {
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
            let inode = inode_read!(current_inode);
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
            let target = inode_read!(target_inode);
            if matches!(target.content, FileContent::Dir(_)) {
                return Err(FsError::IsADirectory);
            }
        }

        // Remove from parent directory
        let mut parent = inode_write!(current_inode);
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
        // Note: The inode will be automatically cleaned up when the Arc/Rc ref count reaches 0
    }

    #[cfg(not(feature = "std"))]
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
            let inode = inode_read!(current_inode);
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
            let target = inode_read!(target_inode);
            if matches!(target.content, FileContent::Dir(_)) {
                return Err(FsError::IsADirectory);
            }
        }

        // Remove from parent directory
        let mut parent = inode_write!(current_inode);
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
        // Note: The inode will be automatically cleaned up when the Arc/Rc ref count reaches 0
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
        let inode = inode_read!(current_inode);
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
        let inode_ref = inode_read!(inode);
        match &inode_ref.content {
            FileContent::Dir(entries) => {
                let mut result = alloc::vec::Vec::new();
                for (name, child_inode_id) in entries.iter() {
                    // Get the child inode to check if it's a directory
                    if let Some(child_inode) = self.inode_table.get(child_inode_id) {
                        let is_dir = inode_read!(child_inode).metadata.is_dir;
                        result.push((name.clone(), is_dir));
                    }
                }
                Ok(result)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    /// Remove an empty directory
    #[cfg(feature = "std")]
    pub fn rmdir(&self, path: &str) -> Result<(), FsError> {
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
            let inode = inode_read!(current_inode);
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
            let target = inode_read!(target_inode);
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
        let mut parent = inode_write!(current_inode);
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
        // Note: The inode will be automatically cleaned up when the Arc/Rc ref count reaches 0
    }

    /// Remove an empty directory
    #[cfg(not(feature = "std"))]
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
            let inode = inode_read!(current_inode);
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
            let target = inode_read!(target_inode);
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
        let mut parent = inode_write!(current_inode);
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
        // Note: The inode will be automatically cleaned up when the Arc/Rc ref count reaches 0
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
