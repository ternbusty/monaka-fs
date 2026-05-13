// VFS Adapter: Minimal WASI filesystem adapter using fs-core
//
// This is a thin adapter component that exports WASI filesystem interfaces
// and delegates to fs-core for the actual filesystem implementation.
//
// Initial filesystem content can be embedded using monaka-virt tool.
//
// Optional S3 sync feature enables automatic synchronization with S3.

#![cfg_attr(not(test), no_main)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
#[cfg(feature = "s3-sync")]
use std::collections::BTreeSet;
use std::ptr::addr_of;
use std::rc::Rc;

use fs_core::snapshot::FsSnapshot;
use fs_core::{Fd, Fs, FsError};

// S3 sync module (only when feature enabled)
#[cfg(feature = "s3-sync")]
mod s3_sync;

// WIT bindgen generates the bindings
// Use different world based on feature
#[cfg(not(feature = "s3-sync"))]
wit_bindgen::generate!({
    world: "vfs-adapter",
    path: "../../../wit",
    generate_all,
});

#[cfg(feature = "s3-sync")]
wit_bindgen::generate!({
    world: "vfs-adapter-s3",
    path: "../../../wit",
    generate_all,
});

// Re-export for convenience
use exports::wasi::filesystem::types::{
    Descriptor, DescriptorBorrow, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    DirectoryEntryStream, ErrorCode, Filesize, NewTimestamp, OpenFlags, PathFlags,
};

// System time provider for fs-core
struct SystemTimeProvider;

impl fs_core::TimeProvider for SystemTimeProvider {
    fn now(&self) -> u64 {
        // Use wall clock import
        let datetime = wasi::clocks::wall_clock::now();
        datetime.seconds
    }
}

// Main VFS adapter state held in thread-local cells. WASI components run in
// a single-threaded environment, so RefCell access is safe and we avoid the
// `static mut` lint trap.
thread_local! {
    static VFS_STATE: RefCell<Option<VfsState>> = const { RefCell::new(None) };
    // Separate cell for the FS itself to avoid re-entrancy issues
    static VFS_FS: RefCell<Option<Rc<RefCell<Fs<SystemTimeProvider>>>>> = const { RefCell::new(None) };
}

// Runtime-injected snapshot data (set by monaka-pack CLI).
// These remain `static mut` because the CLI patches the binary at the named
// symbol addresses. They are read via `addr_of!` to avoid creating a
// reference to a mutable static.
#[no_mangle]
#[used]
static mut MONAKA_FS_FS_DATA_PTR: u32 = 0;

#[no_mangle]
#[used]
static mut MONAKA_FS_FS_DATA_LEN: u32 = 0;

/// Try to load the runtime-injected snapshot from memory
fn load_runtime_snapshot() -> Option<FsSnapshot> {
    let ptr = unsafe { addr_of!(MONAKA_FS_FS_DATA_PTR).read() };
    let len = unsafe { addr_of!(MONAKA_FS_FS_DATA_LEN).read() };

    if ptr == 0 || len == 0 {
        return None;
    }

    // Read data from memory
    let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };

    // Parse JSON snapshot
    serde_json::from_slice::<FsSnapshot>(data).ok()
}

/// Load snapshot from runtime injection (set by monaka-pack)
fn load_snapshot() -> Option<FsSnapshot> {
    load_runtime_snapshot()
}

struct VfsState {
    fs: Rc<RefCell<Fs<SystemTimeProvider>>>,
    // Map descriptor handle to FD
    descriptor_to_fd: BTreeMap<u32, Fd>,
    // Map FD to descriptor handle
    fd_to_descriptor: BTreeMap<Fd, u32>,
    // Map descriptor handle to path (for S3 sync)
    #[cfg(feature = "s3-sync")]
    descriptor_to_path: BTreeMap<u32, String>,
    // Track which descriptors have been written to (for S3 sync on close)
    #[cfg(feature = "s3-sync")]
    dirty_descriptors: BTreeSet<u32>,
    next_descriptor: u32,
}

impl VfsState {
    fn new() -> Self {
        // Try to load snapshot (runtime-injected or compile-time embedded)
        let fs = if let Some(snapshot) = load_snapshot() {
            Rc::new(RefCell::new(Fs::from_snapshot(
                snapshot,
                SystemTimeProvider,
            )))
        } else {
            Rc::new(RefCell::new(Fs::with_time_provider(SystemTimeProvider)))
        };

        // Initialize S3 sync if enabled
        #[cfg(feature = "s3-sync")]
        s3_sync::init_s3_sync(fs.clone());

        let mut state = Self {
            fs,
            descriptor_to_fd: BTreeMap::new(),
            fd_to_descriptor: BTreeMap::new(),
            #[cfg(feature = "s3-sync")]
            descriptor_to_path: BTreeMap::new(),
            #[cfg(feature = "s3-sync")]
            dirty_descriptors: BTreeSet::new(),
            next_descriptor: 1, // 0 is reserved for root
        };

        // Register root directory as descriptor 0
        // Root directory in fs-core doesn't have a FD, but we need to handle it specially
        // We'll use FD 0 as a special marker for root
        state.descriptor_to_fd.insert(0, 0);
        state.fd_to_descriptor.insert(0, 0);

        state
    }
}

impl VfsState {
    fn allocate_descriptor(&mut self, fd: Fd) -> u32 {
        let desc = self.next_descriptor;
        self.next_descriptor += 1;
        self.descriptor_to_fd.insert(desc, fd);
        self.fd_to_descriptor.insert(fd, desc);
        desc
    }

    #[cfg(feature = "s3-sync")]
    fn allocate_descriptor_with_path(&mut self, fd: Fd, path: String) -> u32 {
        let desc = self.allocate_descriptor(fd);
        self.descriptor_to_path.insert(desc, path);
        desc
    }

    #[cfg(feature = "s3-sync")]
    fn get_path(&self, descriptor: u32) -> Option<&String> {
        self.descriptor_to_path.get(&descriptor)
    }

    #[cfg(feature = "s3-sync")]
    fn mark_dirty(&mut self, descriptor: u32) {
        self.dirty_descriptors.insert(descriptor);
    }

    #[cfg(feature = "s3-sync")]
    fn sync_if_dirty(&mut self, descriptor: u32) {
        if self.dirty_descriptors.remove(&descriptor) {
            if let Some(path) = self.descriptor_to_path.get(&descriptor) {
                s3_sync::on_write(path);
            }
        }
    }

    fn get_fd(&self, descriptor: u32) -> Result<Fd, ErrorCode> {
        self.descriptor_to_fd
            .get(&descriptor)
            .copied()
            .ok_or(ErrorCode::BadDescriptor)
    }

    fn release_descriptor(&mut self, descriptor: u32) {
        if let Some(fd) = self.descriptor_to_fd.remove(&descriptor) {
            self.fd_to_descriptor.remove(&fd);
        }
        #[cfg(feature = "s3-sync")]
        self.descriptor_to_path.remove(&descriptor);
    }
}

// Ensure VFS state is initialized once.
fn ensure_init() {
    VFS_STATE.with(|state_cell| {
        if state_cell.borrow().is_none() {
            let state = VfsState::new();
            VFS_FS.with(|fs_cell| {
                *fs_cell.borrow_mut() = Some(state.fs.clone());
            });
            *state_cell.borrow_mut() = Some(state);
        }
    });
}

// Helper to get or initialize VFS state
fn with_vfs_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut VfsState) -> R,
{
    ensure_init();
    VFS_STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        f(borrow.as_mut().unwrap())
    })
}

// Helper to get VFS FS (for use in stream implementations to avoid re-entrancy).
// Returns a clone of the `Rc` so callers do not have to keep the thread-local
// borrow alive while operating on the filesystem.
fn get_vfs_fs() -> Rc<RefCell<Fs<SystemTimeProvider>>> {
    ensure_init();
    VFS_FS.with(|cell| cell.borrow().as_ref().unwrap().clone())
}

// Convert fs-core error to WASI error code
fn to_error_code(err: FsError) -> ErrorCode {
    match err {
        FsError::NotFound => ErrorCode::NoEntry,
        FsError::NotADirectory => ErrorCode::NotDirectory,
        FsError::IsADirectory => ErrorCode::IsDirectory,
        FsError::InvalidArgument => ErrorCode::Invalid,
        FsError::BadFileDescriptor => ErrorCode::BadDescriptor,
        FsError::PermissionDenied => ErrorCode::Access,
        FsError::AlreadyExists => ErrorCode::Exist,
        FsError::NotEmpty => ErrorCode::NotEmpty,
    }
}

// Normalise a relative path coming from a WASI caller into the absolute form
// fs-core expects (`/foo/bar`).
fn normalize_path(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

// Build a `DescriptorStat` from the given type and fs-core metadata,
// populating WASI timestamps from the metadata's `created` / `modified` fields.
fn make_descriptor_stat(type_: DescriptorType, metadata: &fs_core::Metadata) -> DescriptorStat {
    let to_datetime = |secs: u64| wasi::clocks::wall_clock::Datetime {
        seconds: secs,
        nanoseconds: 0,
    };
    DescriptorStat {
        type_,
        link_count: 1,
        size: metadata.size,
        data_access_timestamp: Some(to_datetime(metadata.created)),
        data_modification_timestamp: Some(to_datetime(metadata.modified)),
        status_change_timestamp: Some(to_datetime(metadata.modified)),
    }
}

// Convert WASI flags to fs-core flags
fn convert_flags(open_flags: OpenFlags, descriptor_flags: DescriptorFlags) -> u32 {
    let mut flags = 0u32;

    // Access mode
    if descriptor_flags.contains(DescriptorFlags::READ)
        && descriptor_flags.contains(DescriptorFlags::WRITE)
    {
        flags |= fs_core::O_RDWR;
    } else if descriptor_flags.contains(DescriptorFlags::WRITE) {
        flags |= fs_core::O_WRONLY;
    } else {
        flags |= fs_core::O_RDONLY;
    }

    // Open flags
    if open_flags.contains(OpenFlags::CREATE) {
        flags |= fs_core::O_CREAT;
    }
    if open_flags.contains(OpenFlags::TRUNCATE) {
        flags |= fs_core::O_TRUNC;
    }
    // Note: exclusive and directory flags need special handling

    flags
}

// Export the preopens interface
export!(VfsAdapter);

struct VfsAdapter;

impl exports::wasi::filesystem::preopens::Guest for VfsAdapter {
    fn get_directories() -> Vec<(Descriptor, String)> {
        // Create a proper Descriptor resource for the root directory
        let desc = Descriptor::new(DescriptorImpl { handle: 0 });
        vec![(desc, "/".to_string())]
    }
}

impl exports::wasi::filesystem::types::Guest for VfsAdapter {
    type Descriptor = DescriptorImpl;
    type DirectoryEntryStream = DirectoryEntryStreamImpl;

    fn filesystem_error_code(_err: exports::wasi::io::error::ErrorBorrow<'_>) -> Option<ErrorCode> {
        // Not yet implemented. Would convert io error resource to filesystem error code
        // This function is used to downcast stream errors to filesystem errors
        None
    }
}

// Descriptor resource implementation
struct DescriptorImpl {
    handle: u32,
}

// Implement Drop to properly close file descriptors when the resource is dropped
impl Drop for DescriptorImpl {
    fn drop(&mut self) {
        // Don't close the root directory (handle 0)
        if self.handle == 0 {
            return;
        }

        with_vfs_state(|state| {
            // Sync to S3 if this descriptor was written to
            #[cfg(feature = "s3-sync")]
            state.sync_if_dirty(self.handle);

            if let Some(fd) = state.descriptor_to_fd.get(&self.handle).copied() {
                // Close the fd in fs-core
                let _ = state.fs.borrow_mut().close(fd);
            }
            // Release the descriptor from our mappings
            state.release_descriptor(self.handle);
        });
    }
}

impl exports::wasi::filesystem::types::GuestDescriptor for DescriptorImpl {
    fn read_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::InputStream, ErrorCode> {
        let handle = self.handle;
        let (fd, _path) = with_vfs_state(|state| {
            let fd = state.get_fd(handle)?;
            #[cfg(feature = "s3-sync")]
            let path = state.get_path(handle).map(|s| s.to_string());
            #[cfg(not(feature = "s3-sync"))]
            let path: Option<String> = None;
            Ok((fd, path))
        })?;

        // Create InputStream using enum design (wasi-virt style)
        Ok(exports::wasi::io::streams::InputStream::new(
            VfsInputStream::File {
                fd,
                offset: Cell::new(offset),
                #[cfg(feature = "s3-sync")]
                path: _path,
                #[cfg(feature = "s3-sync")]
                s3_refreshed: Cell::new(false),
            },
        ))
    }

    fn write_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        let handle = self.handle;
        let (fd, _path) = with_vfs_state(|state| {
            let fd = state.get_fd(handle)?;
            #[cfg(feature = "s3-sync")]
            let path = state.get_path(handle).map(|s| s.to_string());
            #[cfg(not(feature = "s3-sync"))]
            let path: Option<String> = None;
            Ok((fd, path))
        })?;

        // Create OutputStream using enum design (wasi-virt style)
        Ok(exports::wasi::io::streams::OutputStream::new(
            VfsOutputStream::File {
                fd,
                offset: Cell::new(offset),
                #[cfg(feature = "s3-sync")]
                path: _path,
                #[cfg(feature = "s3-sync")]
                dirty: Cell::new(false),
            },
        ))
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        let handle = self.handle;
        let (fd, offset, _path) = with_vfs_state(|state| {
            let fd = state.get_fd(handle)?;

            // Get file size for append mode
            let offset = state
                .fs
                .borrow_mut()
                .seek(fd, 0, 2) // SEEK_END
                .map_err(to_error_code)? as u64;

            #[cfg(feature = "s3-sync")]
            let path = state.get_path(handle).map(|s| s.to_string());
            #[cfg(not(feature = "s3-sync"))]
            let path: Option<String> = None;

            Ok((fd, offset, path))
        })?;

        // Create OutputStream using enum design (wasi-virt style)
        Ok(exports::wasi::io::streams::OutputStream::new(
            VfsOutputStream::File {
                fd,
                offset: Cell::new(offset),
                #[cfg(feature = "s3-sync")]
                path: _path,
                #[cfg(feature = "s3-sync")]
                dirty: Cell::new(false),
            },
        ))
    }

    fn advise(
        &self,
        _offset: Filesize,
        _length: Filesize,
        _advice: exports::wasi::filesystem::types::Advice,
    ) -> Result<(), ErrorCode> {
        // Advisory operations are no-ops for in-memory filesystem
        Ok(())
    }

    fn sync_data(&self) -> Result<(), ErrorCode> {
        // No-op for in-memory filesystem
        Ok(())
    }

    fn get_flags(&self) -> Result<DescriptorFlags, ErrorCode> {
        // Return default flags for now
        Ok(DescriptorFlags::READ | DescriptorFlags::WRITE)
    }

    fn get_type(&self) -> Result<DescriptorType, ErrorCode> {
        with_vfs_state(|state| {
            if self.handle == 0 {
                return Ok(DescriptorType::Directory);
            }

            let fd = state.get_fd(self.handle)?;

            // Get metadata from fs
            let metadata = state.fs.borrow_mut().fstat(fd).map_err(to_error_code)?;

            // Determine type based on metadata
            if metadata.is_dir {
                Ok(DescriptorType::Directory)
            } else {
                Ok(DescriptorType::RegularFile)
            }
        })
    }

    fn set_size(&self, size: Filesize) -> Result<(), ErrorCode> {
        with_vfs_state(|state| {
            let fd = state.get_fd(self.handle)?;

            state
                .fs
                .borrow_mut()
                .ftruncate(fd, size)
                .map_err(to_error_code)?;

            // Mark descriptor as dirty - sync will happen on close
            #[cfg(feature = "s3-sync")]
            state.mark_dirty(self.handle);

            Ok(())
        })
    }

    fn set_times(
        &self,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        // Not yet implemented
        Err(ErrorCode::Unsupported)
    }

    fn read(&self, length: Filesize, offset: Filesize) -> Result<(Vec<u8>, bool), ErrorCode> {
        with_vfs_state(|state| {
            let fd = state.get_fd(self.handle)?;

            // Notify S3 sync to refresh from S3 before read (if read-through mode)
            #[cfg(feature = "s3-sync")]
            if let Some(path) = state.get_path(self.handle) {
                s3_sync::on_read(path);
            }

            // Seek to offset
            state
                .fs
                .borrow_mut()
                .seek(fd, offset as i64, 0) // SEEK_SET
                .map_err(to_error_code)?;

            // Read data
            let mut buf = vec![0u8; length as usize];
            let n = state
                .fs
                .borrow_mut()
                .read(fd, &mut buf)
                .map_err(to_error_code)?;

            buf.truncate(n);
            let end_of_stream = n < length as usize;

            Ok((buf, end_of_stream))
        })
    }

    fn write(&self, buffer: Vec<u8>, offset: Filesize) -> Result<Filesize, ErrorCode> {
        with_vfs_state(|state| {
            let fd = state.get_fd(self.handle)?;

            // Seek to offset
            state
                .fs
                .borrow_mut()
                .seek(fd, offset as i64, 0) // SEEK_SET
                .map_err(to_error_code)?;

            // Write data
            let n = state
                .fs
                .borrow_mut()
                .write(fd, &buffer)
                .map_err(to_error_code)?;

            // Mark descriptor as dirty - sync will happen on close
            #[cfg(feature = "s3-sync")]
            state.mark_dirty(self.handle);

            Ok(n as Filesize)
        })
    }

    fn read_directory(&self) -> Result<DirectoryEntryStream, ErrorCode> {
        with_vfs_state(|state| {
            // Special case for root directory (handle=0 uses fake fd not in fd_table)
            let mut entries: Vec<(String, bool)> = if self.handle == 0 {
                // readdir returns Vec<String>, need to convert to Vec<(String, bool)>
                let names = state.fs.borrow().readdir("/").map_err(to_error_code)?;
                names
                    .into_iter()
                    .map(|name| {
                        let path = format!("/{}", name);
                        let is_dir = state
                            .fs
                            .borrow()
                            .stat(&path)
                            .map(|m| m.is_dir)
                            .unwrap_or(false);
                        (name, is_dir)
                    })
                    .collect()
            } else {
                let fd = state.get_fd(self.handle)?;
                state.fs.borrow().readdir_fd(fd).map_err(to_error_code)?
            };

            // Add "." and ".." entries for Unix compatibility
            // These are standard directory entries that should always be present
            let mut full_entries = vec![
                (".".to_string(), true),  // "." is always a directory
                ("..".to_string(), true), // ".." is always a directory
            ];
            full_entries.append(&mut entries);

            // Create directory entry stream
            let stream_impl = DirectoryEntryStreamImpl {
                entries: full_entries,
                index: Cell::new(0),
            };

            Ok(DirectoryEntryStream::new(stream_impl))
        })
    }

    fn sync(&self) -> Result<(), ErrorCode> {
        // No-op for in-memory filesystem
        Ok(())
    }

    fn create_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        with_vfs_state(|state| {
            // For now, we don't have directory FDs properly implemented
            // This is a limitation we'll address later
            // Just create using absolute path for root
            if self.handle == 0 {
                // Use mkdir (not mkdir_p) to properly return errors for existing directories
                // fs::create_dir_all() will call this multiple times for nested paths
                state
                    .fs
                    .borrow_mut()
                    .mkdir(&normalize_path(&path))
                    .map_err(to_error_code)
            } else {
                Err(ErrorCode::Unsupported)
            }
        })
    }

    fn stat(&self) -> Result<DescriptorStat, ErrorCode> {
        with_vfs_state(|state| {
            if self.handle == 0 {
                let metadata = state.fs.borrow_mut().stat("/").map_err(to_error_code)?;
                return Ok(make_descriptor_stat(DescriptorType::Directory, &metadata));
            }

            let fd = state.get_fd(self.handle)?;
            let metadata = state.fs.borrow_mut().fstat(fd).map_err(to_error_code)?;

            let type_ = if metadata.is_dir {
                DescriptorType::Directory
            } else {
                DescriptorType::RegularFile
            };

            Ok(make_descriptor_stat(type_, &metadata))
        })
    }

    fn stat_at(&self, _path_flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        with_vfs_state(|state| {
            let full_path = if self.handle == 0 {
                normalize_path(&path)
            } else {
                return Err(ErrorCode::Unsupported);
            };

            let metadata = state
                .fs
                .borrow_mut()
                .stat(&full_path)
                .map_err(to_error_code)?;

            let type_ = if metadata.is_dir {
                DescriptorType::Directory
            } else {
                DescriptorType::RegularFile
            };

            Ok(make_descriptor_stat(type_, &metadata))
        })
    }

    fn set_times_at(
        &self,
        _path_flags: PathFlags,
        _path: String,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn link_at(
        &self,
        _old_path_flags: PathFlags,
        _old_path: String,
        _new_descriptor: DescriptorBorrow<'_>,
        _new_path: String,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn open_at(
        &self,
        _path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        flags: DescriptorFlags,
    ) -> Result<Descriptor, ErrorCode> {
        // Special case: opening ".", "", or "/" from root means opening the root directory
        if path.is_empty() || path == "." || (self.handle == 0 && path == "/") {
            // Actually open the root directory to get a real fd
            return with_vfs_state(|state| {
                let fd = state
                    .fs
                    .borrow_mut()
                    .open_path_with_flags("/", fs_core::O_RDONLY)
                    .map_err(to_error_code)?;
                let handle = state.allocate_descriptor(fd);
                Ok(Descriptor::new(DescriptorImpl { handle }))
            });
        }

        with_vfs_state(|state| {
            let dir_fd = state.get_fd(self.handle)?;

            let core_flags = convert_flags(open_flags, flags);

            // Use open_at if available, otherwise fall back to absolute path for root.
            // `full_path` is only consumed when s3-sync is enabled.
            #[cfg_attr(not(feature = "s3-sync"), allow(unused_variables))]
            let (fd, full_path) = if self.handle == 0 {
                // Root directory: use absolute path
                let full_path = normalize_path(&path);
                let fd = state
                    .fs
                    .borrow_mut()
                    .open_path_with_flags(&full_path, core_flags)
                    .map_err(to_error_code)?;
                (fd, full_path)
            } else {
                // Use open_at
                let fd = state
                    .fs
                    .borrow_mut()
                    .open_at(dir_fd, &path, core_flags)
                    .map_err(to_error_code)?;
                (fd, path.clone())
            };

            #[cfg(feature = "s3-sync")]
            let handle = state.allocate_descriptor_with_path(fd, full_path.clone());
            #[cfg(not(feature = "s3-sync"))]
            let handle = state.allocate_descriptor(fd);

            // Notify S3 sync to check metadata on open (if metadata sync mode)
            #[cfg(feature = "s3-sync")]
            s3_sync::on_open(&full_path);

            Ok(Descriptor::new(DescriptorImpl { handle }))
        })
    }

    fn readlink_at(&self, _path: String) -> Result<String, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn remove_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        with_vfs_state(|state| {
            // For root descriptor, use absolute path
            let full_path = if self.handle == 0 {
                normalize_path(&path)
            } else {
                return Err(ErrorCode::Unsupported);
            };

            // Use rmdir for removing directories
            state
                .fs
                .borrow_mut()
                .rmdir(&full_path)
                .map_err(to_error_code)
        })
    }

    fn rename_at(
        &self,
        old_path: String,
        _new_descriptor: DescriptorBorrow<'_>,
        new_path: String,
    ) -> Result<(), ErrorCode> {
        with_vfs_state(|state| {
            let old_full = if self.handle == 0 {
                normalize_path(&old_path)
            } else {
                return Err(ErrorCode::Unsupported);
            };
            let new_full = normalize_path(&new_path);
            state
                .fs
                .borrow_mut()
                .rename(&old_full, &new_full)
                .map_err(to_error_code)
        })
    }

    fn symlink_at(&self, _old_path: String, _new_path: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn unlink_file_at(&self, path: String) -> Result<(), ErrorCode> {
        with_vfs_state(|state| {
            // For root descriptor, use absolute path
            let full_path = if self.handle == 0 {
                normalize_path(&path)
            } else {
                return Err(ErrorCode::Unsupported);
            };

            state
                .fs
                .borrow_mut()
                .unlink(&full_path)
                .map_err(to_error_code)?;

            // Notify S3 sync of the deletion
            #[cfg(feature = "s3-sync")]
            s3_sync::on_delete(&full_path);

            Ok(())
        })
    }

    fn is_same_object(&self, other: DescriptorBorrow<'_>) -> bool {
        self.handle == other.get::<DescriptorImpl>().handle
    }

    fn metadata_hash(
        &self,
    ) -> Result<exports::wasi::filesystem::types::MetadataHashValue, ErrorCode> {
        with_vfs_state(|state| {
            let fd = state.get_fd(self.handle)?;
            let metadata = state.fs.borrow_mut().fstat(fd).map_err(to_error_code)?;

            // Use inode information to create a hash value
            // For simplicity, we'll use the file size and timestamps
            Ok(exports::wasi::filesystem::types::MetadataHashValue {
                lower: metadata.size,
                upper: metadata.modified,
            })
        })
    }

    fn metadata_hash_at(
        &self,
        _path_flags: PathFlags,
        path: String,
    ) -> Result<exports::wasi::filesystem::types::MetadataHashValue, ErrorCode> {
        with_vfs_state(|state| {
            let full_path = if self.handle == 0 {
                normalize_path(&path)
            } else {
                return Err(ErrorCode::Unsupported);
            };

            let metadata = state
                .fs
                .borrow_mut()
                .stat(&full_path)
                .map_err(to_error_code)?;

            // Use file information to create a hash value
            Ok(exports::wasi::filesystem::types::MetadataHashValue {
                lower: metadata.size,
                upper: metadata.modified,
            })
        })
    }
}

// DirectoryEntryStream resource implementation
struct DirectoryEntryStreamImpl {
    entries: Vec<(String, bool)>, // (name, is_dir)
    index: Cell<usize>,
}

impl exports::wasi::filesystem::types::GuestDirectoryEntryStream for DirectoryEntryStreamImpl {
    fn read_directory_entry(&self) -> Result<Option<DirectoryEntry>, ErrorCode> {
        let idx = self.index.get();
        if idx >= self.entries.len() {
            return Ok(None);
        }

        let (name, is_dir) = self.entries[idx].clone();
        self.index.set(idx + 1);

        // Determine type from the is_dir flag
        let type_ = if is_dir {
            DescriptorType::Directory
        } else {
            DescriptorType::RegularFile
        };

        Ok(Some(DirectoryEntry { type_, name }))
    }
}

// InputStream implementation for wasi:io/streams
// Using enum design like wasi-virt
pub enum VfsInputStream {
    File {
        fd: Fd,
        offset: Cell<u64>,
        #[cfg(feature = "s3-sync")]
        path: Option<String>,
        #[cfg(feature = "s3-sync")]
        s3_refreshed: Cell<bool>,
    },
    Host(wasi::io::streams::InputStream),
}

impl exports::wasi::io::streams::GuestInputStream for VfsInputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        match self {
            #[cfg(feature = "s3-sync")]
            Self::File {
                fd,
                offset,
                path,
                s3_refreshed,
            } => {
                // Refresh from S3 before read (once per stream, on first read)
                if !s3_refreshed.get() {
                    if let Some(p) = path {
                        s3_sync::on_read(p);
                    }
                    s3_refreshed.set(true);
                }

                let fs = get_vfs_fs();
                let current_offset = offset.get();

                // Seek to offset
                fs.borrow_mut()
                    .seek(*fd, current_offset as i64, 0) // SEEK_SET
                    .map_err(|_| {
                        exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                            exports::wasi::io::error::Error::from_handle(0)
                        })
                    })?;

                // Read from fs-core
                let mut buf = vec![0u8; len as usize];
                let n = fs.borrow_mut().read(*fd, &mut buf).map_err(|_| {
                    exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                        exports::wasi::io::error::Error::from_handle(0)
                    })
                })?;

                buf.truncate(n);
                offset.set(current_offset + n as u64);

                Ok(buf)
            }
            #[cfg(not(feature = "s3-sync"))]
            Self::File { fd, offset } => {
                let fs = get_vfs_fs();
                let current_offset = offset.get();

                // Seek to offset
                fs.borrow_mut()
                    .seek(*fd, current_offset as i64, 0) // SEEK_SET
                    .map_err(|_| {
                        exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                            exports::wasi::io::error::Error::from_handle(0)
                        })
                    })?;

                // Read from fs-core
                let mut buf = vec![0u8; len as usize];
                let n = fs.borrow_mut().read(*fd, &mut buf).map_err(|_| {
                    exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                        exports::wasi::io::error::Error::from_handle(0)
                    })
                })?;

                buf.truncate(n);
                offset.set(current_offset + n as u64);

                Ok(buf)
            }
            Self::Host(stream) => stream.read(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        self.read(len)
    }

    fn skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            Self::File { offset, .. } => {
                offset.set(offset.get() + len);
                Ok(len)
            }
            Self::Host(stream) => stream.skip(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        self.skip(len)
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        match self {
            Self::File { .. } => {
                // Return immediately ready pollable for in-memory FS
                exports::wasi::io::poll::Pollable::new(PollableImpl)
            }
            Self::Host(_stream) => {
                // Wrap host pollable. For now just return ready pollable
                // TODO: properly wrap host pollable
                exports::wasi::io::poll::Pollable::new(PollableImpl)
            }
        }
    }
}

// OutputStream implementation for wasi:io/streams
// Using enum design like wasi-virt
pub enum VfsOutputStream {
    File {
        fd: Fd,
        offset: Cell<u64>,
        #[cfg(feature = "s3-sync")]
        path: Option<String>,
        #[cfg(feature = "s3-sync")]
        dirty: Cell<bool>,
    },
    Host(wasi::io::streams::OutputStream),
}

/// Sync to S3 when output stream is dropped (file closed)
#[cfg(feature = "s3-sync")]
impl Drop for VfsOutputStream {
    fn drop(&mut self) {
        if let Self::File { path, dirty, .. } = self {
            // Only sync if we actually wrote something
            if dirty.get() {
                if let Some(p) = path {
                    s3_sync::on_write(p);
                }
            }
        }
    }
}

impl exports::wasi::io::streams::GuestOutputStream for VfsOutputStream {
    fn check_write(&self) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            Self::File { .. } => {
                // Always allow up to 4KB for in-memory FS
                Ok(4096)
            }
            Self::Host(stream) => stream.check_write().map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn write(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            #[cfg(feature = "s3-sync")]
            Self::File {
                fd,
                offset,
                path: _,
                dirty,
            } => {
                let fs = get_vfs_fs();
                let current_offset = offset.get();

                // Seek to offset
                fs.borrow_mut()
                    .seek(*fd, current_offset as i64, 0) // SEEK_SET
                    .map_err(|_| {
                        exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                            exports::wasi::io::error::Error::from_handle(0)
                        })
                    })?;

                // Write to fs-core
                let n = fs.borrow_mut().write(*fd, &contents).map_err(|_| {
                    exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                        exports::wasi::io::error::Error::from_handle(0)
                    })
                })?;

                offset.set(current_offset + n as u64);

                // Mark as dirty - sync will happen on drop
                dirty.set(true);

                Ok(())
            }
            #[cfg(not(feature = "s3-sync"))]
            Self::File { fd, offset } => {
                let fs = get_vfs_fs();
                let current_offset = offset.get();

                // Seek to offset
                fs.borrow_mut()
                    .seek(*fd, current_offset as i64, 0) // SEEK_SET
                    .map_err(|_| {
                        exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                            exports::wasi::io::error::Error::from_handle(0)
                        })
                    })?;

                // Write to fs-core
                let n = fs.borrow_mut().write(*fd, &contents).map_err(|_| {
                    exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                        exports::wasi::io::error::Error::from_handle(0)
                    })
                })?;

                offset.set(current_offset + n as u64);

                Ok(())
            }
            Self::Host(stream) => stream.write(&contents).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_write_and_flush(
        &self,
        contents: Vec<u8>,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.write(contents)?;
        self.flush()?;
        Ok(())
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            Self::File { .. } => {
                // No-op for in-memory FS
                Ok(())
            }
            Self::Host(stream) => stream.flush().map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.flush()
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        match self {
            Self::File { .. } => exports::wasi::io::poll::Pollable::new(PollableImpl),
            Self::Host(_stream) => {
                // TODO: properly wrap host pollable
                exports::wasi::io::poll::Pollable::new(PollableImpl)
            }
        }
    }

    fn write_zeroes(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            Self::File { .. } => {
                let zeroes = vec![0u8; len as usize];
                self.write(zeroes)
            }
            Self::Host(stream) => stream.write_zeroes(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_write_zeroes_and_flush(
        &self,
        len: u64,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.write_zeroes(len)?;
        self.flush()?;
        Ok(())
    }

    fn splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        // Splice not implemented
        Err(exports::wasi::io::streams::StreamError::Closed)
    }

    fn blocking_splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(exports::wasi::io::streams::StreamError::Closed)
    }
}

// Pollable stub implementation
struct PollableImpl;

impl exports::wasi::io::poll::GuestPollable for PollableImpl {
    fn ready(&self) -> bool {
        true
    }

    fn block(&self) {
        // No-op for in-memory FS
    }
}

// Error resource implementation
struct ErrorImpl;

impl exports::wasi::io::error::GuestError for ErrorImpl {
    fn to_debug_string(&self) -> String {
        "vfs error".to_string()
    }
}

// Implement Guest trait for wasi:io/error
impl exports::wasi::io::error::Guest for VfsAdapter {
    type Error = ErrorImpl;
}

// Implement Guest trait for wasi:io/streams
impl exports::wasi::io::streams::Guest for VfsAdapter {
    type InputStream = VfsInputStream;
    type OutputStream = VfsOutputStream;
}

// Implement Guest trait for wasi:io/poll
impl exports::wasi::io::poll::Guest for VfsAdapter {
    type Pollable = PollableImpl;

    fn poll(pollables: Vec<exports::wasi::io::poll::PollableBorrow<'_>>) -> Vec<u32> {
        // For in-memory FS, all operations are ready immediately
        (0..pollables.len() as u32).collect()
    }
}

// Implement Guest trait for wasi:cli/stdin (passthrough)
impl exports::wasi::cli::stdin::Guest for VfsAdapter {
    fn get_stdin() -> exports::wasi::cli::stdin::InputStream {
        // Wrap host's stdin using enum design
        let host_stdin = wasi::cli::stdin::get_stdin();
        exports::wasi::io::streams::InputStream::new(VfsInputStream::Host(host_stdin))
    }
}

// Implement Guest trait for wasi:cli/stdout (passthrough)
impl exports::wasi::cli::stdout::Guest for VfsAdapter {
    fn get_stdout() -> exports::wasi::cli::stdout::OutputStream {
        // Wrap host's stdout using enum design
        let host_stdout = wasi::cli::stdout::get_stdout();
        exports::wasi::io::streams::OutputStream::new(VfsOutputStream::Host(host_stdout))
    }
}

// Implement Guest trait for wasi:cli/stderr (passthrough)
impl exports::wasi::cli::stderr::Guest for VfsAdapter {
    fn get_stderr() -> exports::wasi::cli::stderr::OutputStream {
        // Wrap host's stderr using enum design
        let host_stderr = wasi::cli::stderr::get_stderr();
        exports::wasi::io::streams::OutputStream::new(VfsOutputStream::Host(host_stderr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_adds_leading_slash() {
        assert_eq!(normalize_path("foo/bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_keeps_single_leading_slash() {
        assert_eq!(normalize_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_collapses_repeated_leading_slashes() {
        assert_eq!(normalize_path("///foo"), "/foo");
    }

    #[test]
    fn normalize_path_handles_empty() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn to_error_code_maps_all_variants() {
        assert!(matches!(
            to_error_code(FsError::NotFound),
            ErrorCode::NoEntry
        ));
        assert!(matches!(
            to_error_code(FsError::NotADirectory),
            ErrorCode::NotDirectory
        ));
        assert!(matches!(
            to_error_code(FsError::IsADirectory),
            ErrorCode::IsDirectory
        ));
        assert!(matches!(
            to_error_code(FsError::InvalidArgument),
            ErrorCode::Invalid
        ));
        assert!(matches!(
            to_error_code(FsError::BadFileDescriptor),
            ErrorCode::BadDescriptor
        ));
        assert!(matches!(
            to_error_code(FsError::PermissionDenied),
            ErrorCode::Access
        ));
        assert!(matches!(
            to_error_code(FsError::AlreadyExists),
            ErrorCode::Exist
        ));
        assert!(matches!(
            to_error_code(FsError::NotEmpty),
            ErrorCode::NotEmpty
        ));
    }

    #[test]
    fn make_descriptor_stat_carries_metadata_timestamps() {
        let metadata = fs_core::Metadata {
            size: 4096,
            created: 100,
            modified: 200,
            permissions: 0o644,
            is_dir: true,
        };
        let stat = make_descriptor_stat(DescriptorType::Directory, &metadata);
        assert!(matches!(stat.type_, DescriptorType::Directory));
        assert_eq!(stat.size, 4096);
        assert_eq!(stat.link_count, 1);
        assert_eq!(stat.data_access_timestamp.unwrap().seconds, 100);
        assert_eq!(stat.data_modification_timestamp.unwrap().seconds, 200);
        assert_eq!(stat.status_change_timestamp.unwrap().seconds, 200);
    }

    #[test]
    fn convert_flags_rdonly_when_only_read_requested() {
        let f = convert_flags(OpenFlags::empty(), DescriptorFlags::READ);
        assert_eq!(f & 0x03, fs_core::O_RDONLY);
    }

    #[test]
    fn convert_flags_wronly_when_only_write_requested() {
        let f = convert_flags(OpenFlags::empty(), DescriptorFlags::WRITE);
        assert_eq!(f & 0x03, fs_core::O_WRONLY);
    }

    #[test]
    fn convert_flags_rdwr_when_both_requested() {
        let f = convert_flags(
            OpenFlags::empty(),
            DescriptorFlags::READ | DescriptorFlags::WRITE,
        );
        assert_eq!(f & 0x03, fs_core::O_RDWR);
    }

    #[test]
    fn convert_flags_passes_create_and_truncate() {
        let f = convert_flags(
            OpenFlags::CREATE | OpenFlags::TRUNCATE,
            DescriptorFlags::WRITE,
        );
        assert!(f & fs_core::O_CREAT != 0);
        assert!(f & fs_core::O_TRUNC != 0);
    }
}
