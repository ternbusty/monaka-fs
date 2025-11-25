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
        // Create a proper Descriptor resource for the root directory
        let desc = Descriptor::new(DescriptorImpl { handle: 0 });
        vec![(desc, "/".to_string())]
    }
}

impl exports::wasi::filesystem::types::Guest for VfsProvider {
    type Descriptor = DescriptorImpl;
    type DirectoryEntryStream = DirectoryEntryStreamImpl;

    fn filesystem_error_code(_err: exports::wasi::io::error::ErrorBorrow<'_>) -> Option<ErrorCode> {
        // Not yet implemented - would convert io error resource to filesystem error code
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
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            // Create InputStream implementation
            let stream_impl = InputStreamImpl {
                fd,
                offset: RefCell::new(offset),
            };

            // Register as wasi:io/streams resource and return handle
            Ok(exports::wasi::io::streams::InputStream::new(stream_impl))
        })
    }

    fn write_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            // Create OutputStream implementation
            let stream_impl = OutputStreamImpl {
                fd,
                offset: RefCell::new(offset),
            };

            // Register as wasi:io/streams resource and return handle
            Ok(exports::wasi::io::streams::OutputStream::new(stream_impl))
        })
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let fd = state.get_fd(self.handle)?;

            // Get file size for append mode
            let offset = state
                .fs
                .borrow_mut()
                .seek(fd, 0, 2) // SEEK_END
                .map_err(to_error_code)? as u64;

            // Create OutputStream implementation
            let stream_impl = OutputStreamImpl {
                fd,
                offset: RefCell::new(offset),
            };

            // Register as wasi:io/streams resource and return handle
            Ok(exports::wasi::io::streams::OutputStream::new(stream_impl))
        })
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

        
            
            if self.handle == 0 {
                return Ok(DescriptorType::Directory);
            }

            let fd = state.get_fd(self.handle)?;

            // Get metadata from fs
            let _metadata = state.fs.borrow().fstat(fd).map_err(to_error_code)?;

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

        
            
            if self.handle == 0 {
                let metadata = state.fs.borrow().stat("/").map_err(to_error_code)?;
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
            let metadata = state.fs.borrow().fstat(fd).map_err(to_error_code)?;

            Ok(DescriptorStat {
                type_: DescriptorType::RegularFile, // TODO: get from metadata
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

        
            
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                return Err(ErrorCode::Unsupported);
            };

            let metadata = state.fs.borrow().stat(&full_path).map_err(to_error_code)?;

            Ok(DescriptorStat {
                type_: DescriptorType::RegularFile, // TODO: determine from metadata
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

            let handle = state.allocate_descriptor(fd);
            Ok(Descriptor::new(DescriptorImpl { handle }))
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

// InputStream implementation for wasi:io/streams
struct InputStreamImpl {
    fd: Fd,
    offset: RefCell<u64>,
}

impl exports::wasi::io::streams::GuestInputStream for InputStreamImpl {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let offset = *self.offset.borrow();

            // Seek to offset
            state
                .fs
                .borrow_mut()
                .seek(self.fd, offset as i64, 0) // SEEK_SET
                .map_err(|_| exports::wasi::io::streams::StreamError::LastOperationFailed(
                    unsafe { exports::wasi::io::error::Error::from_handle(0) }
                ))?;

            // Read from fs-core
            let mut buf = vec![0u8; len as usize];
            let n = state
                .fs
                .borrow_mut()
                .read(self.fd, &mut buf)
                .map_err(|_| exports::wasi::io::streams::StreamError::LastOperationFailed(
                    unsafe { exports::wasi::io::error::Error::from_handle(0) }
                ))?;

            buf.truncate(n);
            *self.offset.borrow_mut() += n as u64;

            Ok(buf)
        })
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        self.read(len)
    }

    fn skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        *self.offset.borrow_mut() += len;
        Ok(len)
    }

    fn blocking_skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        self.skip(len)
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        // Return immediately ready pollable for in-memory FS
        exports::wasi::io::poll::Pollable::new(PollableImpl)
    }
}

// OutputStream implementation for wasi:io/streams
struct OutputStreamImpl {
    fd: Fd,
    offset: RefCell<u64>,
}

impl exports::wasi::io::streams::GuestOutputStream for OutputStreamImpl {
    fn check_write(&self) -> Result<u64, exports::wasi::io::streams::StreamError> {
        // Always allow up to 4KB for in-memory FS
        Ok(4096)
    }

    fn write(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        VFS_STATE.with(|state| {
            let state = state.borrow();
            let offset = *self.offset.borrow();

            // Seek to offset
            state
                .fs
                .borrow_mut()
                .seek(self.fd, offset as i64, 0) // SEEK_SET
                .map_err(|_| exports::wasi::io::streams::StreamError::LastOperationFailed(
                    unsafe { exports::wasi::io::error::Error::from_handle(0) }
                ))?;

            // Write to fs-core
            let n = state
                .fs
                .borrow_mut()
                .write(self.fd, &contents)
                .map_err(|_| exports::wasi::io::streams::StreamError::LastOperationFailed(
                    unsafe { exports::wasi::io::error::Error::from_handle(0) }
                ))?;

            *self.offset.borrow_mut() += n as u64;

            Ok(())
        })
    }

    fn blocking_write_and_flush(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.write(contents)?;
        self.flush()?;
        Ok(())
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        // No-op for in-memory FS
        Ok(())
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        Ok(())
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        exports::wasi::io::poll::Pollable::new(PollableImpl)
    }

    fn write_zeroes(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        let zeroes = vec![0u8; len as usize];
        self.write(zeroes)
    }

    fn blocking_write_zeroes_and_flush(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.write_zeroes(len)?;
        self.flush()?;
        Ok(())
    }

    fn splice(&self, _src: exports::wasi::io::streams::InputStreamBorrow<'_>, _len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        // Splice not implemented
        Err(exports::wasi::io::streams::StreamError::Closed)
    }

    fn blocking_splice(&self, _src: exports::wasi::io::streams::InputStreamBorrow<'_>, _len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
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
impl exports::wasi::io::error::Guest for VfsProvider {
    type Error = ErrorImpl;
}

// Implement Guest trait for wasi:io/streams
impl exports::wasi::io::streams::Guest for VfsProvider {
    type InputStream = InputStreamImpl;
    type OutputStream = OutputStreamImpl;
}

// Implement Guest trait for wasi:io/poll
impl exports::wasi::io::poll::Guest for VfsProvider {
    type Pollable = PollableImpl;

    fn poll(pollables: Vec<exports::wasi::io::poll::PollableBorrow<'_>>) -> Vec<u32> {
        // For in-memory FS, all operations are ready immediately
        (0..pollables.len() as u32).collect()
    }
}
