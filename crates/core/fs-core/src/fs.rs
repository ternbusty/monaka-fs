//! Filesystem implementation.
//!
//! A single source of truth used in two modes via the `thread-safe` feature:
//!
//! - `thread-safe` (host runtimes, requires `std`): backed by `DashMap` and
//!   atomic counters, with `Arc<RwLock<Inode>>` references. The whole API is
//!   `&self` so multiple wasmtime hosts can share one `Fs` across threads.
//! - default (WASI components or no-std embedders): backed by
//!   `RefCell<HashMap>` (or `RefCell<BTreeMap>` in no-std) with `Cell<u64>`
//!   counters and `Rc<RefCell<Inode>>` references. The API is still `&self`
//!   thanks to interior mutability.
//!
//! Storage differences are isolated to a thin set of accessor helpers
//! (`fd_with`, `inode_get`, `allocate_*`, ...) near the top of the file.
//! Every public method then has a single body that compiles for both
//! flavours.

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap;

#[cfg(feature = "thread-safe")]
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
#[cfg(feature = "thread-safe")]
use std::sync::{Arc, RwLock};

#[cfg(feature = "thread-safe")]
use dashmap::DashMap;

#[cfg(all(feature = "std", not(feature = "thread-safe")))]
use std::cell::{Cell, RefCell};
#[cfg(all(feature = "std", not(feature = "thread-safe")))]
use std::collections::HashMap;
#[cfg(all(feature = "std", not(feature = "thread-safe")))]
use std::rc::Rc;

#[cfg(not(feature = "std"))]
use alloc::rc::Rc;
#[cfg(not(feature = "std"))]
use core::cell::{Cell, RefCell};

use crate::error::FsError;
use crate::handle::FileHandle;
use crate::inode::{FileContent, Inode, Metadata};
use crate::time::{MonotonicCounter, TimeProvider};
use crate::types::*;

// Import logging macros (no-ops when the `logging` feature is off).
#[cfg(feature = "logging")]
use crate::{debug, error, trace};

// =============================================================================
// Storage type aliases
// =============================================================================

/// A reference to an `Inode` shared between the table and any open handles.
///
/// Thread-safe builds use `Arc<RwLock<Inode>>` so multiple readers can hold
/// a guard at once. Single-thread builds use `Rc<RefCell<Inode>>`.
#[cfg(feature = "thread-safe")]
pub type InodeRef = Arc<RwLock<Inode>>;
#[cfg(not(feature = "thread-safe"))]
pub type InodeRef = Rc<RefCell<Inode>>;

/// Table from `Fd` to `FileHandle`. `DashMap` in thread-safe mode (lock-free
/// concurrent access across shards), plain map under a `RefCell` otherwise.
#[cfg(feature = "thread-safe")]
type FdTable = DashMap<Fd, FileHandle>;
#[cfg(all(feature = "std", not(feature = "thread-safe")))]
type FdTable = RefCell<HashMap<Fd, FileHandle>>;
#[cfg(not(feature = "std"))]
type FdTable = RefCell<BTreeMap<Fd, FileHandle>>;

/// Table from `InodeId` to `InodeRef`.
#[cfg(feature = "thread-safe")]
type InodeTable = DashMap<InodeId, InodeRef>;
#[cfg(all(feature = "std", not(feature = "thread-safe")))]
type InodeTable = RefCell<HashMap<InodeId, InodeRef>>;
#[cfg(not(feature = "std"))]
type InodeTable = RefCell<BTreeMap<InodeId, InodeRef>>;

/// Counter for the next free inode ID.
#[cfg(feature = "thread-safe")]
type NextInodeCounter = AtomicU64;
#[cfg(not(feature = "thread-safe"))]
type NextInodeCounter = Cell<InodeId>;

/// Counter for the next free file descriptor.
#[cfg(feature = "thread-safe")]
type NextFdCounter = AtomicU32;
#[cfg(not(feature = "thread-safe"))]
type NextFdCounter = Cell<Fd>;

// =============================================================================
// Inode access macros
// =============================================================================

/// Read-locks an inode and yields a guard that derefs to `&Inode`.
macro_rules! inode_read {
    ($inode:expr) => {{
        #[cfg(feature = "thread-safe")]
        {
            $inode.read().unwrap()
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            $inode.borrow()
        }
    }};
}

/// Write-locks an inode and yields a guard that derefs to `&mut Inode`.
macro_rules! inode_write {
    ($inode:expr) => {{
        #[cfg(feature = "thread-safe")]
        {
            $inode.write().unwrap()
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            $inode.borrow_mut()
        }
    }};
}

// =============================================================================
// Fs struct
// =============================================================================

/// Main filesystem structure. See the module-level comment for an overview
/// of the two storage flavours.
pub struct Fs<T: TimeProvider = MonotonicCounter> {
    pub(crate) next_inode: NextInodeCounter,
    pub(crate) next_fd: NextFdCounter,
    pub(crate) fd_table: FdTable,
    pub(crate) inode_table: InodeTable,
    pub(crate) root_inode: InodeId,
    pub(crate) time_provider: T,
}

impl<T: TimeProvider> Fs<T> {
    // -------------------------------------------------------------------------
    // Construction / accessors
    // -------------------------------------------------------------------------

    /// Create a new inode reference suitable for insertion in `inode_table`.
    pub(crate) fn new_inode_ref(inode: Inode) -> InodeRef {
        #[cfg(feature = "thread-safe")]
        {
            Arc::new(RwLock::new(inode))
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            Rc::new(RefCell::new(inode))
        }
    }

    /// Build an empty filesystem with the supplied time provider. A root
    /// directory is created automatically with timestamps populated by the
    /// provider.
    pub fn with_time_provider(time_provider: T) -> Self {
        let timestamp = time_provider.now();
        let mut root_inode = Inode::new_dir(0);
        root_inode.metadata.created = timestamp;
        root_inode.metadata.modified = timestamp;

        let fs = Self::empty_with(time_provider, 0, 1);
        fs.inode_insert(0, Self::new_inode_ref(root_inode));
        fs
    }

    /// Build an empty Fs with no inodes and no fd entries. Used by snapshot
    /// restoration, which then populates `inode_table` from the snapshot's
    /// inode list.
    pub(crate) fn empty_with(
        time_provider: T,
        root_inode: InodeId,
        next_inode_seed: InodeId,
    ) -> Self {
        Self {
            next_inode: new_inode_counter(next_inode_seed),
            next_fd: new_fd_counter(3), // 0/1/2 reserved for stdin/out/err
            fd_table: new_fd_table(),
            inode_table: new_inode_table(),
            root_inode,
            time_provider,
        }
    }

    fn allocate_inode(&self) -> InodeId {
        #[cfg(feature = "thread-safe")]
        {
            self.next_inode.fetch_add(1, Ordering::Relaxed) as InodeId
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            let id = self.next_inode.get();
            self.next_inode.set(id + 1);
            id
        }
    }

    fn allocate_fd(&self) -> Fd {
        #[cfg(feature = "thread-safe")]
        {
            self.next_fd.fetch_add(1, Ordering::Relaxed)
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            let fd = self.next_fd.get();
            self.next_fd.set(fd + 1);
            fd
        }
    }

    // -------------------------------------------------------------------------
    // fd_table helpers
    // -------------------------------------------------------------------------

    /// Run `f` with access to the `FileHandle` for `fd`. Returns
    /// `BadFileDescriptor` if the fd is unknown.
    ///
    /// The closure holds the underlying borrow / shard lock for its full
    /// duration, so it should not call back into `Fs` methods that mutate
    /// the same table (this would `panic!` under `RefCell` and could
    /// deadlock under `DashMap`).
    fn fd_with<R>(&self, fd: Fd, f: impl FnOnce(&FileHandle) -> R) -> Result<R, FsError> {
        #[cfg(feature = "thread-safe")]
        {
            let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
            Ok(f(&handle))
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            let map = self.fd_table.borrow();
            let handle = map.get(&fd).ok_or(FsError::BadFileDescriptor)?;
            Ok(f(handle))
        }
    }

    /// Like `fd_with` but returns `None` (not an error) when the fd is
    /// missing. Useful for "best effort" position updates.
    fn fd_try_with<R>(&self, fd: Fd, f: impl FnOnce(&FileHandle) -> R) -> Option<R> {
        #[cfg(feature = "thread-safe")]
        {
            self.fd_table.get(&fd).map(|h| f(&h))
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            self.fd_table.borrow().get(&fd).map(f)
        }
    }

    fn fd_insert(&self, fd: Fd, handle: FileHandle) {
        #[cfg(feature = "thread-safe")]
        {
            self.fd_table.insert(fd, handle);
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            self.fd_table.borrow_mut().insert(fd, handle);
        }
    }

    fn fd_remove(&self, fd: Fd) -> Option<FileHandle> {
        #[cfg(feature = "thread-safe")]
        {
            self.fd_table.remove(&fd).map(|(_, v)| v)
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            self.fd_table.borrow_mut().remove(&fd)
        }
    }

    // -------------------------------------------------------------------------
    // inode_table helpers
    // -------------------------------------------------------------------------

    /// Look up an inode by id. Returns a clone of the (cheap) `Arc`/`Rc`
    /// reference rather than borrowing the table, so callers don't need to
    /// worry about borrow lifetimes.
    pub(crate) fn inode_get(&self, id: InodeId) -> Option<InodeRef> {
        #[cfg(feature = "thread-safe")]
        {
            self.inode_table.get(&id).map(|r| r.clone())
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            self.inode_table.borrow().get(&id).cloned()
        }
    }

    pub(crate) fn inode_insert(&self, id: InodeId, inode: InodeRef) {
        #[cfg(feature = "thread-safe")]
        {
            self.inode_table.insert(id, inode);
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            self.inode_table.borrow_mut().insert(id, inode);
        }
    }

    /// Visit every inode in the table. Used by snapshot serialization.
    /// Holds the inode table lock for the duration of iteration.
    pub(crate) fn inode_for_each<F>(&self, mut f: F)
    where
        F: FnMut(InodeId, &InodeRef),
    {
        #[cfg(feature = "thread-safe")]
        {
            for entry in self.inode_table.iter() {
                f(*entry.key(), entry.value());
            }
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            for (&id, inode_rc) in self.inode_table.borrow().iter() {
                f(id, inode_rc);
            }
        }
    }

    /// Current value of the next-inode counter (for snapshot serialization).
    pub(crate) fn next_inode_value(&self) -> InodeId {
        #[cfg(feature = "thread-safe")]
        {
            self.next_inode.load(Ordering::Relaxed)
        }
        #[cfg(not(feature = "thread-safe"))]
        {
            self.next_inode.get()
        }
    }

    // -------------------------------------------------------------------------
    // Internal path / inode helpers
    // -------------------------------------------------------------------------

    fn find_inode(&self, parent_inode: &InodeRef, name: &str) -> Result<InodeRef, FsError> {
        let parent = inode_read!(parent_inode);

        match &parent.content {
            FileContent::Dir(entries) => {
                if let Some(&inode_id) = entries.get(name) {
                    self.inode_get(inode_id).ok_or(FsError::NotFound)
                } else {
                    Err(FsError::NotFound)
                }
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

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
                self.inode_insert(new_inode_id, new_inode_ref.clone());
                parent.metadata.modified = timestamp;

                Ok(new_inode_ref)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    // -------------------------------------------------------------------------
    // Public API
    // -------------------------------------------------------------------------

    pub fn open_path(&self, path: &str) -> Result<Fd, FsError> {
        self.open_path_with_flags(path, O_RDWR | O_CREAT)
    }

    pub fn open_at(&self, dir_fd: Fd, path: &str, flags: u32) -> Result<Fd, FsError> {
        debug!(
            "open_at: dir_fd={}, path={}, flags={:#x}",
            dir_fd, path, flags
        );

        if path.starts_with('/') {
            error!("open_at: absolute path not allowed");
            return Err(FsError::InvalidArgument);
        }

        let dir_inode_id = self.fd_with(dir_fd, |h| h.inode_id).map_err(|_| {
            error!("open_at: bad directory file descriptor {}", dir_fd);
            FsError::BadFileDescriptor
        })?;

        let dir_inode = self.inode_get(dir_inode_id).ok_or(FsError::NotFound)?;

        // Verify it's a directory
        {
            let inode = inode_read!(dir_inode);
            if matches!(inode.content, FileContent::File(_)) {
                error!("open_at: dir_fd {} is not a directory", dir_fd);
                return Err(FsError::NotADirectory);
            }
        }

        if path.is_empty() {
            error!("open_at: empty path not supported yet");
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode = dir_inode;

        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;
            let inode = inode_read!(current_inode);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let filename = comps[comps.len() - 1];
        let file_inode = match self.find_inode(&current_inode, filename) {
            Ok(inode) => inode,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                self.create_inode(&current_inode, filename, false)?
            }
            Err(e) => return Err(e),
        };

        // POSIX: opening a directory with write access or O_TRUNC is not allowed.
        {
            let inode = inode_read!(file_inode);
            if matches!(inode.content, FileContent::Dir(_)) {
                let access_mode = flags & 0x3;
                if access_mode == O_WRONLY || access_mode == O_RDWR {
                    return Err(FsError::IsADirectory);
                }
                if flags & O_TRUNC != 0 {
                    return Err(FsError::IsADirectory);
                }
            }
        }

        // O_TRUNC: truncate file to 0 bytes (POSIX requires write permission)
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
        self.fd_insert(fd, handle);
        debug!("open_at: allocated fd={} for inode={}", fd, inode_id);
        Ok(fd)
    }

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
            self.fd_insert(fd, handle);
            debug!("open_path_with_flags: allocated fd={} for root", fd);
            return Ok(fd);
        }

        let root_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;
        let mut current_inode = root_inode;

        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;
            let inode = inode_read!(current_inode);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let filename = comps[comps.len() - 1];
        let file_inode = match self.find_inode(&current_inode, filename) {
            Ok(inode) => inode,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                self.create_inode(&current_inode, filename, false)?
            }
            Err(e) => return Err(e),
        };

        {
            let inode = inode_read!(file_inode);
            if matches!(inode.content, FileContent::Dir(_)) {
                let access_mode = flags & 0x3;
                if access_mode == O_WRONLY || access_mode == O_RDWR {
                    return Err(FsError::IsADirectory);
                }
                if flags & O_TRUNC != 0 {
                    return Err(FsError::IsADirectory);
                }
            }
        }

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
        self.fd_insert(fd, handle);
        debug!(
            "open_path_with_flags: allocated fd={} for inode={}",
            fd, inode_id
        );
        Ok(fd)
    }

    pub fn write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        trace!("write: fd={}, len={}", fd, buf.len());

        let (inode_id, flags, position) = self
            .fd_with(fd, |h| (h.inode_id, h.flags, h.get_position()))
            .map_err(|_| {
                error!("write: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;

        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                // O_APPEND: write at end of file
                let pos = if flags & O_APPEND != 0 {
                    storage.size()
                } else {
                    position as usize
                };

                let n = storage.write(pos, buf);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();

                // Release inode lock before re-acquiring the fd entry.
                drop(inode_ref);
                self.fd_try_with(fd, |h| h.set_position((pos + n) as u64));

                debug!("write: fd={}, wrote {} bytes at pos {}", fd, n, pos);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("write: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    /// Write data at a specific offset atomically (seek + write in one
    /// lock acquisition). Does not touch O_APPEND.
    pub fn write_at(&self, fd: Fd, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        trace!("write_at: fd={}, offset={}, len={}", fd, offset, buf.len());

        let (inode_id, flags) = self.fd_with(fd, |h| (h.inode_id, h.flags)).map_err(|_| {
            error!("write_at: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                let n = storage.write(offset as usize, buf);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();

                drop(inode_ref);
                self.fd_try_with(fd, |h| h.set_position(offset + n as u64));

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

    /// Append data to file atomically (always writes at end of file).
    pub fn append_write(&self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        trace!("append_write: fd={}, len={}", fd, buf.len());

        let (inode_id, flags) = self.fd_with(fd, |h| (h.inode_id, h.flags)).map_err(|_| {
            error!("append_write: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode_write!(inode);

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                let pos = storage.size();
                let n = storage.write(pos, buf);
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();

                drop(inode_ref);
                self.fd_try_with(fd, |h| h.set_position((pos + n) as u64));

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

    pub fn read(&self, fd: Fd, out: &mut [u8]) -> Result<usize, FsError> {
        trace!("read: fd={}, buf_len={}", fd, out.len());

        let (inode_id, flags, position) = self
            .fd_with(fd, |h| (h.inode_id, h.flags, h.get_position()))
            .map_err(|_| {
                error!("read: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;

        let access_mode = flags & 0x3;
        if access_mode == O_WRONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
        let inode_ref = inode_read!(inode);

        match &inode_ref.content {
            FileContent::File(storage) => {
                let pos = position as usize;
                let n = storage.read(pos, out);
                drop(inode_ref);
                self.fd_try_with(fd, |h| h.set_position(position + n as u64));
                debug!("read: fd={}, read {} bytes at pos {}", fd, n, pos);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("read: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    /// Read data at a specific offset atomically (seek + read in one
    /// lock acquisition).
    pub fn read_at(&self, fd: Fd, offset: u64, out: &mut [u8]) -> Result<usize, FsError> {
        trace!(
            "read_at: fd={}, offset={}, buf_len={}",
            fd,
            offset,
            out.len()
        );

        let (inode_id, flags) = self.fd_with(fd, |h| (h.inode_id, h.flags)).map_err(|_| {
            error!("read_at: bad file descriptor {}", fd);
            FsError::BadFileDescriptor
        })?;

        let access_mode = flags & 0x3;
        if access_mode == O_WRONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
        let inode_ref = inode_read!(inode);

        match &inode_ref.content {
            FileContent::File(storage) => {
                let n = storage.read(offset as usize, out);
                drop(inode_ref);
                self.fd_try_with(fd, |h| h.set_position(offset + n as u64));
                debug!("read_at: fd={}, read {} bytes at offset {}", fd, n, offset);
                Ok(n)
            }
            FileContent::Dir(_) => {
                error!("read_at: fd={} is a directory", fd);
                Err(FsError::BadFileDescriptor)
            }
        }
    }

    pub fn ftruncate(&self, fd: Fd, size: u64) -> Result<(), FsError> {
        let (inode_id, flags) = self.fd_with(fd, |h| (h.inode_id, h.flags))?;

        let access_mode = flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
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

    pub fn close(&self, fd: Fd) -> Result<(), FsError> {
        trace!("close: fd={}", fd);

        self.fd_remove(fd).ok_or_else(|| {
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
            let root = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;
            return Ok(inode_read!(root).metadata);
        }

        let mut current_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;

        for comp in comps.iter() {
            current_inode = self.find_inode(&current_inode, comp)?;
        }

        Ok(inode_read!(current_inode).metadata)
    }

    pub fn fstat(&self, fd: Fd) -> Result<Metadata, FsError> {
        let inode_id = self.fd_with(fd, |h| h.inode_id)?;
        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
        Ok(inode_read!(inode).metadata)
    }

    pub fn seek(&self, fd: Fd, offset: i64, whence: i32) -> Result<u64, FsError> {
        trace!("seek: fd={}, offset={}, whence={}", fd, offset, whence);

        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        let (inode_id, current) = self
            .fd_with(fd, |h| (h.inode_id, h.get_position()))
            .map_err(|_| {
                error!("seek: bad file descriptor {}", fd);
                FsError::BadFileDescriptor
            })?;
        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;
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
                        let new = current as i64 + offset;
                        if new < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        new as u64
                    }
                    SEEK_END => {
                        let size = storage.size() as i64;
                        let new = size + offset;
                        if new < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        new as u64
                    }
                    _ => return Err(FsError::InvalidArgument),
                };

                drop(inode_ref);
                self.fd_try_with(fd, |h| h.set_position(new_pos));
                debug!("seek: fd={}, new_pos={}", fd, new_pos);
                Ok(new_pos)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

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

        let mut current_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;

        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;
            let inode = inode_read!(current_inode);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let dirname = comps[comps.len() - 1];
        self.create_inode(&current_inode, dirname, true)?;
        debug!("mkdir: created directory {}", path);
        Ok(())
    }

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

        let mut current_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;

        for comp in comps.iter() {
            current_inode = match self.find_inode(&current_inode, comp) {
                Ok(inode) => {
                    {
                        let borrowed = inode_read!(inode);
                        if matches!(borrowed.content, FileContent::File(_)) {
                            return Err(FsError::NotADirectory);
                        }
                    }
                    inode
                }
                Err(FsError::NotFound) => self.create_inode(&current_inode, comp, true)?,
                Err(e) => return Err(e),
            };
        }

        Ok(())
    }

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
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;

        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;
            let inode = inode_read!(current_inode);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let filename = comps[comps.len() - 1];

        // Verify the target exists and is not a directory.
        let target_inode = self.find_inode(&current_inode, filename)?;
        {
            let target = inode_read!(target_inode);
            if matches!(target.content, FileContent::Dir(_)) {
                return Err(FsError::IsADirectory);
            }
        }

        // Remove the entry from the parent directory.
        let mut parent = inode_write!(current_inode);
        match &mut parent.content {
            FileContent::Dir(entries) => {
                entries.remove(filename);
                let timestamp = self.time_provider.now();
                parent.metadata.modified = timestamp;
                debug!("unlink: removed {}", path);
                Ok(())
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
        // The inode is cleaned up when the last `Arc`/`Rc` ref drops.
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

        let mut current_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;

        for comp in comps.iter() {
            current_inode = self.find_inode(&current_inode, comp)?;
        }

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

    /// Read directory entries from an open file descriptor.
    /// Returns a list of `(name, is_dir)` tuples.
    pub fn readdir_fd(
        &self,
        fd: Fd,
    ) -> Result<alloc::vec::Vec<(alloc::string::String, bool)>, FsError> {
        let inode_id = self.fd_with(fd, |h| h.inode_id)?;
        let inode = self.inode_get(inode_id).ok_or(FsError::NotFound)?;

        // Snapshot the (name, child_id) pairs so we don't hold the inode
        // lock while looking up child inodes (which would conflict with a
        // hypothetical concurrent writer holding both).
        let entries: alloc::vec::Vec<(alloc::string::String, InodeId)> = {
            let inode_ref = inode_read!(inode);
            match &inode_ref.content {
                FileContent::Dir(map) => map.iter().map(|(n, &id)| (n.clone(), id)).collect(),
                FileContent::File(_) => return Err(FsError::NotADirectory),
            }
        };

        let mut result = alloc::vec::Vec::with_capacity(entries.len());
        for (name, child_id) in entries {
            if let Some(child_inode) = self.inode_get(child_id) {
                let is_dir = inode_read!(child_inode).metadata.is_dir;
                result.push((name, is_dir));
            }
        }
        Ok(result)
    }

    /// Remove an empty directory.
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
            return Err(FsError::InvalidArgument);
        }

        let mut current_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;

        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;
            let inode = inode_read!(current_inode);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let dirname = comps[comps.len() - 1];
        let target_inode = self.find_inode(&current_inode, dirname)?;

        {
            let target = inode_read!(target_inode);
            match &target.content {
                FileContent::File(_) => return Err(FsError::NotADirectory),
                FileContent::Dir(entries) => {
                    if !entries.is_empty() {
                        error!("rmdir: directory not empty");
                        return Err(FsError::NotEmpty);
                    }
                }
            }
        }

        let mut parent = inode_write!(current_inode);
        match &mut parent.content {
            FileContent::Dir(entries) => {
                entries.remove(dirname);
                let timestamp = self.time_provider.now();
                parent.metadata.modified = timestamp;
                debug!("rmdir: removed {}", path);
                Ok(())
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    /// Rename or move a file or directory.
    pub fn rename(&self, old_path: &str, new_path: &str) -> Result<(), FsError> {
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
            return Err(FsError::InvalidArgument);
        }

        if old_comps == new_comps {
            return Ok(());
        }

        // Navigate to old parent directory.
        let root_inode = self.inode_get(self.root_inode).ok_or(FsError::NotFound)?;
        let mut old_parent = root_inode.clone();

        for comp in old_comps.iter().take(old_comps.len() - 1) {
            old_parent = self.find_inode(&old_parent, comp)?;
            let inode = inode_read!(old_parent);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let old_name = old_comps[old_comps.len() - 1];

        // Verify source exists and get its type.
        let source_inode = self.find_inode(&old_parent, old_name)?;
        let source_is_dir = inode_read!(source_inode).metadata.is_dir;

        // Navigate to new parent directory.
        let mut new_parent = root_inode;

        for comp in new_comps.iter().take(new_comps.len() - 1) {
            new_parent = self.find_inode(&new_parent, comp)?;
            let inode = inode_read!(new_parent);
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        let new_name = new_comps[new_comps.len() - 1];

        // If destination exists, validate type compatibility.
        if let Ok(dest_inode) = self.find_inode(&new_parent, new_name) {
            let dest = inode_read!(dest_inode);
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

        let old_parent_id = inode_read!(old_parent).id;
        let new_parent_id = inode_read!(new_parent).id;

        let timestamp = self.time_provider.now();

        if old_parent_id == new_parent_id {
            // Same directory: single lock, remove + insert.
            let mut parent = inode_write!(old_parent);
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
            // Cross-directory: lock both parents in inode id order to avoid
            // deadlock under thread-safe builds.
            if old_parent_id < new_parent_id {
                let mut old_p = inode_write!(old_parent);
                let mut new_p = inode_write!(new_parent);
                let old_entries = match &mut old_p.content {
                    FileContent::Dir(e) => e,
                    _ => return Err(FsError::NotADirectory),
                };
                let inode_id = old_entries.remove(old_name).ok_or(FsError::NotFound)?;
                old_p.metadata.modified = timestamp;
                let new_entries = match &mut new_p.content {
                    FileContent::Dir(e) => e,
                    _ => return Err(FsError::NotADirectory),
                };
                new_entries.insert(alloc::string::String::from(new_name), inode_id);
                new_p.metadata.modified = timestamp;
            } else {
                let mut new_p = inode_write!(new_parent);
                let mut old_p = inode_write!(old_parent);
                let old_entries = match &mut old_p.content {
                    FileContent::Dir(e) => e,
                    _ => return Err(FsError::NotADirectory),
                };
                let inode_id = old_entries.remove(old_name).ok_or(FsError::NotFound)?;
                old_p.metadata.modified = timestamp;
                let new_entries = match &mut new_p.content {
                    FileContent::Dir(e) => e,
                    _ => return Err(FsError::NotADirectory),
                };
                new_entries.insert(alloc::string::String::from(new_name), inode_id);
                new_p.metadata.modified = timestamp;
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

// =============================================================================
// Constructor helpers (cfg-gated for the four storage primitives)
// =============================================================================

#[cfg(feature = "thread-safe")]
fn new_fd_table() -> FdTable {
    DashMap::new()
}
#[cfg(all(feature = "std", not(feature = "thread-safe")))]
fn new_fd_table() -> FdTable {
    RefCell::new(HashMap::new())
}
#[cfg(not(feature = "std"))]
fn new_fd_table() -> FdTable {
    RefCell::new(BTreeMap::new())
}

#[cfg(feature = "thread-safe")]
fn new_inode_table() -> InodeTable {
    DashMap::new()
}
#[cfg(all(feature = "std", not(feature = "thread-safe")))]
fn new_inode_table() -> InodeTable {
    RefCell::new(HashMap::new())
}
#[cfg(not(feature = "std"))]
fn new_inode_table() -> InodeTable {
    RefCell::new(BTreeMap::new())
}

#[cfg(feature = "thread-safe")]
fn new_inode_counter(initial: InodeId) -> NextInodeCounter {
    AtomicU64::new(initial)
}
#[cfg(not(feature = "thread-safe"))]
fn new_inode_counter(initial: InodeId) -> NextInodeCounter {
    Cell::new(initial)
}

#[cfg(feature = "thread-safe")]
fn new_fd_counter(initial: Fd) -> NextFdCounter {
    AtomicU32::new(initial)
}
#[cfg(not(feature = "thread-safe"))]
fn new_fd_counter(initial: Fd) -> NextFdCounter {
    Cell::new(initial)
}
