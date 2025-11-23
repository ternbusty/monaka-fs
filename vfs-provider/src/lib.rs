// VFS Provider - WASI filesystem implementation using fs-core
//
// This component provides an in-memory VFS that implements WASI Preview 2
// filesystem interfaces.

#![allow(warnings)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fs_core::{Fd, Fs, FsError};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "vfs-provider",
    path: "../wit",
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

// Main VFS provider state
thread_local! {
    static VFS_STATE: RefCell<VfsState> = RefCell::new(VfsState::new());
}

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
export!(VfsProvider);

struct VfsProvider;

impl exports::wasi::filesystem::preopens::Guest for VfsProvider {
    fn get_directories() -> Vec<(Descriptor, String)> {
        // Return root directory as descriptor 0
        unsafe { vec![(Descriptor::from_handle(0), "/".to_string())] }
    }
}

impl exports::wasi::filesystem::types::Guest for VfsProvider {
    type Descriptor = DescriptorImpl;
    type DirectoryEntryStream = DirectoryEntryStreamImpl;
    type Error = ErrorImpl;

    fn filesystem_error_code(
        _err: exports::wasi::filesystem::types::ErrorBorrow<'_>,
    ) -> Option<ErrorCode> {
        // Not yet implemented - would convert error resource to error code
        None
    }
}

// Error resource implementation (stub)
struct ErrorImpl;

impl exports::wasi::filesystem::types::GuestError for ErrorImpl {}

// Descriptor resource implementation
struct DescriptorImpl {
    handle: u32,
}

impl exports::wasi::filesystem::types::GuestDescriptor for DescriptorImpl {
    fn read_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::InputStream, ErrorCode> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            // Create a read stream
            // For now, return an error as streams are not yet implemented
            Err(ErrorCode::Unsupported)
        })
    }

    fn write_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        Err(ErrorCode::Unsupported)
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
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            // Get metadata from fs
            let metadata = state.fs.borrow().fstat(fd).map_err(to_error_code)?;

            // Determine type based on metadata
            // For now, assume regular file (we need to add type info to metadata)
            Ok(DescriptorType::RegularFile)
        })
    }

    fn set_size(&self, size: Filesize) -> Result<(), ErrorCode> {
        VFS_STATE.with(|state| {
            let fd = {
                let state = state.borrow();
                state.get_fd(self.handle)?
            };

            state
                .borrow()
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
        VFS_STATE.with(|state| {
            let state = state.borrow();
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
        VFS_STATE.with(|state| {
            let state = state.borrow();
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
        // Not yet implemented
        Err(ErrorCode::Unsupported)
    }

    fn sync(&self) -> Result<(), ErrorCode> {
        // No-op for in-memory filesystem
        Ok(())
    }

    fn create_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            // For now, we don't have directory FDs properly implemented
            // This is a limitation we'll address later
            // Just create using absolute path for root
            if self.handle == 0 {
                state
                    .fs
                    .borrow_mut()
                    .mkdir(&format!("/{}", path))
                    .map_err(to_error_code)
            } else {
                Err(ErrorCode::Unsupported)
            }
        })
    }

    fn stat(&self) -> Result<DescriptorStat, ErrorCode> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            let metadata = state.fs.borrow().fstat(fd).map_err(to_error_code)?;

            Ok(DescriptorStat {
                file_type: DescriptorType::RegularFile, // TODO: get from metadata
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
        VFS_STATE.with(|state| {
            let state = state.borrow();

            // For root descriptor, use absolute path
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                return Err(ErrorCode::Unsupported);
            };

            let metadata = state.fs.borrow().stat(&full_path).map_err(to_error_code)?;

            Ok(DescriptorStat {
                file_type: DescriptorType::RegularFile, // TODO: determine from metadata
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
        VFS_STATE.with(|state| {
            let mut state = state.borrow_mut();
            let dir_fd = state.get_fd(self.handle)?;

            let core_flags = convert_flags(open_flags, flags);

            // Use open_at if available, otherwise fall back to absolute path for root
            let fd = if self.handle == 0 {
                // Root directory - use absolute path
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

            let descriptor = state.allocate_descriptor(fd);
            Ok(unsafe { Descriptor::from_handle(descriptor) })
        })
    }

    fn readlink_at(&self, _path: String) -> Result<String, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn remove_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        VFS_STATE.with(|state| {
            // For root descriptor, use absolute path
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                return Err(ErrorCode::Unsupported);
            };

            // Use unlink (it will check if it's a directory)
            state
                .borrow()
                .fs
                .borrow_mut()
                .unlink(&full_path)
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
        VFS_STATE.with(|state| {
            // For root descriptor, use absolute path
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                return Err(ErrorCode::Unsupported);
            };

            state
                .borrow()
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
        Err(ErrorCode::Unsupported)
    }

    fn metadata_hash_at(
        &self,
        _path_flags: PathFlags,
        _path: String,
    ) -> Result<exports::wasi::filesystem::types::MetadataHashValue, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }
}

// DirectoryEntryStream resource implementation (stub)
struct DirectoryEntryStreamImpl;

impl exports::wasi::filesystem::types::GuestDirectoryEntryStream for DirectoryEntryStreamImpl {
    fn read_directory_entry(&self) -> Result<Option<DirectoryEntry>, ErrorCode> {
        // Not yet implemented
        Err(ErrorCode::Unsupported)
    }
}
