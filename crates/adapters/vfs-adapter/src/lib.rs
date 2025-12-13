// VFS Adapter: Minimal WASI filesystem adapter using fs-core
//
// This is a thin adapter component that exports WASI filesystem interfaces
// and delegates to fs-core for the actual filesystem implementation.

#![no_main]
#![allow(warnings)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use fs_core::{Fd, Fs, FsError};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "vfs-adapter",
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

// Main VFS adapter state
// Using lazy_static for initialization
use std::sync::Once;

static INIT: Once = Once::new();
static mut VFS_STATE: Option<VfsState> = None;
// Separate static for the FS itself to avoid re-entrancy issues
static mut VFS_FS: Option<Rc<RefCell<Fs<SystemTimeProvider>>>> = None;

struct VfsState {
    fs: Rc<RefCell<Fs<SystemTimeProvider>>>,
    // Map descriptor handle to FD
    descriptor_to_fd: BTreeMap<u32, Fd>,
    // Map FD to descriptor handle
    fd_to_descriptor: BTreeMap<Fd, u32>,
    next_descriptor: u32,
}

impl VfsState {
    fn new() -> Self {
        let fs = Rc::new(RefCell::new(Fs::with_time_provider(SystemTimeProvider)));

        let mut state = Self {
            fs,
            descriptor_to_fd: BTreeMap::new(),
            fd_to_descriptor: BTreeMap::new(),
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

    fn get_fd(&self, descriptor: u32) -> Result<Fd, ErrorCode> {
        self.descriptor_to_fd
            .get(&descriptor)
            .copied()
            .ok_or(ErrorCode::BadDescriptor)
    }

    fn get_descriptor(&self, fd: Fd) -> Option<u32> {
        self.fd_to_descriptor.get(&fd).copied()
    }

    fn release_descriptor(&mut self, descriptor: u32) {
        if let Some(fd) = self.descriptor_to_fd.remove(&descriptor) {
            self.fd_to_descriptor.remove(&fd);
        }
    }
}

// Helper to get or initialize VFS state
fn with_vfs_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut VfsState) -> R,
{
    unsafe {
        INIT.call_once(|| {
            let state = VfsState::new();
            VFS_FS = Some(state.fs.clone());
            VFS_STATE = Some(state);
        });
        f(VFS_STATE.as_mut().unwrap())
    }
}

// Helper to get VFS FS (for use in stream implementations to avoid re-entrancy)
fn get_vfs_fs() -> &'static Rc<RefCell<Fs<SystemTimeProvider>>> {
    unsafe {
        INIT.call_once(|| {
            let state = VfsState::new();
            VFS_FS = Some(state.fs.clone());
            VFS_STATE = Some(state);
        });
        VFS_FS.as_ref().unwrap()
    }
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

impl exports::wasi::filesystem::types::GuestDescriptor for DescriptorImpl {
    fn read_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::InputStream, ErrorCode> {
        let fd = with_vfs_state(|state| state.get_fd(self.handle))?;

        // Create InputStream using enum design (wasi-virt style)
        Ok(exports::wasi::io::streams::InputStream::new(
            VfsInputStream::File {
                fd,
                offset: Cell::new(offset),
            },
        ))
    }

    fn write_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        let fd = with_vfs_state(|state| state.get_fd(self.handle))?;

        // Create OutputStream using enum design (wasi-virt style)
        Ok(exports::wasi::io::streams::OutputStream::new(
            VfsOutputStream::File {
                fd,
                offset: Cell::new(offset),
            },
        ))
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        let (fd, offset) = with_vfs_state(|state| {
            let fd = state.get_fd(self.handle)?;

            // Get file size for append mode
            let offset = state
                .fs
                .borrow_mut()
                .seek(fd, 0, 2) // SEEK_END
                .map_err(to_error_code)? as u64;

            Ok((fd, offset))
        })?;

        // Create OutputStream using enum design (wasi-virt style)
        Ok(exports::wasi::io::streams::OutputStream::new(
            VfsOutputStream::File {
                fd,
                offset: Cell::new(offset),
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
                .map_err(to_error_code)
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

            Ok(n as Filesize)
        })
    }

    fn read_directory(&self) -> Result<DirectoryEntryStream, ErrorCode> {
        with_vfs_state(|state| {
            // Get the file descriptor for this handle
            let fd = state.get_fd(self.handle)?;

            // Read directory entries from fs-core using the fd
            let mut entries = state.fs.borrow().readdir_fd(fd).map_err(to_error_code)?;

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
                    .mkdir(&format!("/{}", path.trim_start_matches('/')))
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
                return Ok(DescriptorStat {
                    type_: DescriptorType::Directory,
                    link_count: 1,
                    size: metadata.size,
                    data_access_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                        seconds: metadata.created,
                        nanoseconds: 0,
                    }),
                    data_modification_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                        seconds: metadata.modified,
                        nanoseconds: 0,
                    }),
                    status_change_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                        seconds: metadata.modified,
                        nanoseconds: 0,
                    }),
                });
            }

            let fd = state.get_fd(self.handle)?;
            let metadata = state.fs.borrow_mut().fstat(fd).map_err(to_error_code)?;

            let type_ = if metadata.is_dir {
                DescriptorType::Directory
            } else {
                DescriptorType::RegularFile
            };

            Ok(DescriptorStat {
                type_,
                link_count: 1,
                size: metadata.size,
                data_access_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                    seconds: metadata.created,
                    nanoseconds: 0,
                }),
                data_modification_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                    seconds: metadata.modified,
                    nanoseconds: 0,
                }),
                status_change_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                    seconds: metadata.modified,
                    nanoseconds: 0,
                }),
            })
        })
    }

    fn stat_at(&self, _path_flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        with_vfs_state(|state| {
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
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

            Ok(DescriptorStat {
                type_,
                link_count: 1,
                size: metadata.size,
                data_access_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                    seconds: metadata.created,
                    nanoseconds: 0,
                }),
                data_modification_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                    seconds: metadata.modified,
                    nanoseconds: 0,
                }),
                status_change_timestamp: Some(wasi::clocks::wall_clock::Datetime {
                    seconds: metadata.modified,
                    nanoseconds: 0,
                }),
            })
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
        // Special case: opening "." or "" means opening the directory itself
        if path.is_empty() || path == "." {
            // Return a new descriptor for the same handle (dup)
            return Ok(Descriptor::new(DescriptorImpl {
                handle: self.handle,
            }));
        }

        with_vfs_state(|state| {
            let dir_fd = state.get_fd(self.handle)?;

            let core_flags = convert_flags(open_flags, flags);

            // Use open_at if available, otherwise fall back to absolute path for root
            let fd = if self.handle == 0 {
                // Root directory: use absolute path
                let full_path = format!("/{}", path.trim_start_matches('/'));
                state
                    .fs
                    .borrow_mut()
                    .open_path_with_flags(&full_path, core_flags)
                    .map_err(to_error_code)?
            } else {
                // Use open_at
                state
                    .fs
                    .borrow_mut()
                    .open_at(dir_fd, &path, core_flags)
                    .map_err(to_error_code)?
            };

            let handle = state.allocate_descriptor(fd);
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
                format!("/{}", path.trim_start_matches('/'))
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
        _old_path: String,
        _new_descriptor: DescriptorBorrow<'_>,
        _new_path: String,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn symlink_at(&self, _old_path: String, _new_path: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn unlink_file_at(&self, path: String) -> Result<(), ErrorCode> {
        with_vfs_state(|state| {
            // For root descriptor, use absolute path
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                return Err(ErrorCode::Unsupported);
            };

            state
                .fs
                .borrow_mut()
                .unlink(&full_path)
                .map_err(to_error_code)
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
                format!("/{}", path.trim_start_matches('/'))
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
    File { fd: Fd, offset: Cell<u64> },
    Host(wasi::io::streams::InputStream),
}

impl exports::wasi::io::streams::GuestInputStream for VfsInputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        match self {
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
            Self::Host(stream) => {
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
    File { fd: Fd, offset: Cell<u64> },
    Host(wasi::io::streams::OutputStream),
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
