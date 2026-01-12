// WASI Filesystem Types Host Implementation
//
// Implements wasi:filesystem/types interface using fs-core directly

use super::{FsDescriptorWrapper, FsDirectoryEntryStreamWrapper, VfsHostState};
use bytes::Bytes;
use fs_core::Fs;
use std::sync::Arc;
use wasmtime::component::Resource;
use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode;
use wasmtime_wasi::{
    HostInputStream, HostOutputStream, StreamError, StreamResult, Subscribe, TrappableError,
};

// fs-core open flags
const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_CREAT: u32 = 0o100;
const O_TRUNC: u32 = 0o1000;

// Seek whence constants
const SEEK_SET: i32 = 0;

/// Wrapper for fs-core InputStream that implements HostInputStream
pub struct FsInputStreamWrapper {
    /// Reference to shared VFS (no external lock needed)
    shared_vfs: Arc<Fs>,
    /// The fs-core file descriptor
    fd: u32,
    /// Current offset position
    offset: u64,
}

impl FsInputStreamWrapper {
    pub fn new(shared_vfs: Arc<Fs>, fd: u32, offset: u64) -> Self {
        Self {
            shared_vfs,
            fd,
            offset,
        }
    }
}

/// Wrapper for fs-core OutputStream that implements HostOutputStream
pub struct FsOutputStreamWrapper {
    /// Reference to shared VFS (no external lock needed)
    shared_vfs: Arc<Fs>,
    /// The fs-core file descriptor
    fd: u32,
    /// Current offset position (None means append mode)
    offset: Option<u64>,
}

impl FsOutputStreamWrapper {
    pub fn new(shared_vfs: Arc<Fs>, fd: u32, offset: Option<u64>) -> Self {
        Self {
            shared_vfs,
            fd,
            offset,
        }
    }
}

impl wasmtime_wasi::bindings::sync::filesystem::types::Host for VfsHostState {
    fn filesystem_error_code(
        &mut self,
        err: Resource<anyhow::Error>,
    ) -> Result<Option<ErrorCode>, anyhow::Error> {
        let _error = self.table.get(&err)?;
        Ok(None)
    }

    fn convert_error_code(
        &mut self,
        err: TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    ) -> Result<wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode, anyhow::Error> {
        let nonsync_error = err.downcast()?;

        use wasmtime_wasi::bindings::filesystem::types::ErrorCode as NonSync;
        use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode as Sync;

        let sync_error = match nonsync_error {
            NonSync::Access => Sync::Access,
            NonSync::WouldBlock => Sync::WouldBlock,
            NonSync::Already => Sync::Already,
            NonSync::BadDescriptor => Sync::BadDescriptor,
            NonSync::Busy => Sync::Busy,
            NonSync::Deadlock => Sync::Deadlock,
            NonSync::Quota => Sync::Quota,
            NonSync::Exist => Sync::Exist,
            NonSync::FileTooLarge => Sync::FileTooLarge,
            NonSync::IllegalByteSequence => Sync::IllegalByteSequence,
            NonSync::InProgress => Sync::InProgress,
            NonSync::Interrupted => Sync::Interrupted,
            NonSync::Invalid => Sync::Invalid,
            NonSync::Io => Sync::Io,
            NonSync::IsDirectory => Sync::IsDirectory,
            NonSync::Loop => Sync::Loop,
            NonSync::TooManyLinks => Sync::TooManyLinks,
            NonSync::MessageSize => Sync::MessageSize,
            NonSync::NameTooLong => Sync::NameTooLong,
            NonSync::NoDevice => Sync::NoDevice,
            NonSync::NoEntry => Sync::NoEntry,
            NonSync::NoLock => Sync::NoLock,
            NonSync::InsufficientMemory => Sync::InsufficientMemory,
            NonSync::InsufficientSpace => Sync::InsufficientSpace,
            NonSync::NotDirectory => Sync::NotDirectory,
            NonSync::NotEmpty => Sync::NotEmpty,
            NonSync::NotRecoverable => Sync::NotRecoverable,
            NonSync::Unsupported => Sync::Unsupported,
            NonSync::NoTty => Sync::NoTty,
            NonSync::NoSuchDevice => Sync::NoSuchDevice,
            NonSync::Overflow => Sync::Overflow,
            NonSync::NotPermitted => Sync::NotPermitted,
            NonSync::Pipe => Sync::Pipe,
            NonSync::ReadOnly => Sync::ReadOnly,
            NonSync::InvalidSeek => Sync::InvalidSeek,
            NonSync::TextFileBusy => Sync::TextFileBusy,
            NonSync::CrossDevice => Sync::CrossDevice,
        };

        Ok(sync_error)
    }
}

impl VfsHostState {
    /// Helper: Get fs-core fd and path from host descriptor resource
    fn get_fs_descriptor(
        &self,
        host_desc: &Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<(u32, Option<String>), anyhow::Error> {
        let rep = host_desc.rep();
        let wrapper_resource: Resource<FsDescriptorWrapper> = Resource::new_borrow(rep);
        let wrapper = self
            .table
            .get(&wrapper_resource)
            .map_err(|e| anyhow::anyhow!("Failed to get descriptor from table: {}", e))?;
        Ok((wrapper.fd, wrapper.path.clone()))
    }

    /// Helper: Resolve relative path from directory descriptor
    fn resolve_path(&self, dir_path: &Option<String>, relative_path: &str) -> String {
        match dir_path {
            Some(dir) if dir == "/" => format!("/{}", relative_path.trim_start_matches('/')),
            Some(dir) => format!(
                "{}/{}",
                dir.trim_end_matches('/'),
                relative_path.trim_start_matches('/')
            ),
            None => format!("/{}", relative_path.trim_start_matches('/')),
        }
    }
}

#[async_trait::async_trait]
impl Subscribe for FsInputStreamWrapper {
    async fn ready(&mut self) {
        // For in-memory VFS, streams are always ready
    }
}

impl HostInputStream for FsInputStreamWrapper {
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        let offset = self.offset;

        // Seek to offset
        self.shared_vfs
            .seek(self.fd, offset as i64, SEEK_SET)
            .map_err(|e| {
                StreamError::LastOperationFailed(anyhow::anyhow!("seek failed: {:?}", e))
            })?;

        // Read data
        let mut buf = vec![0u8; size];
        let n = self.shared_vfs.read(self.fd, &mut buf).map_err(|e| {
            StreamError::LastOperationFailed(anyhow::anyhow!("read failed: {:?}", e))
        })?;

        buf.truncate(n);
        self.offset += n as u64;
        Ok(Bytes::from(buf))
    }

    fn skip(&mut self, nelem: usize) -> StreamResult<usize> {
        // Simply advance the offset
        self.offset += nelem as u64;
        Ok(nelem)
    }
}

#[async_trait::async_trait]
impl Subscribe for FsOutputStreamWrapper {
    async fn ready(&mut self) {
        // For in-memory VFS, streams are always ready
    }
}

impl HostOutputStream for FsOutputStreamWrapper {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        match self.offset {
            Some(offset) => {
                // Positioned write: seek to offset first
                self.shared_vfs
                    .seek(self.fd, offset as i64, SEEK_SET)
                    .map_err(|e| {
                        StreamError::LastOperationFailed(anyhow::anyhow!("seek failed: {:?}", e))
                    })?;

                let n = self.shared_vfs.write(self.fd, &bytes).map_err(|e| {
                    StreamError::LastOperationFailed(anyhow::anyhow!("write failed: {:?}", e))
                })?;

                self.offset = Some(offset + n as u64);
            }
            None => {
                // Append mode: use append_write
                self.shared_vfs.append_write(self.fd, &bytes).map_err(|e| {
                    StreamError::LastOperationFailed(anyhow::anyhow!(
                        "append_write failed: {:?}",
                        e
                    ))
                })?;
            }
        }

        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        // In-memory FS: no-op
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        // In-memory FS: can always write
        Ok(1024 * 1024) // 1MB buffer
    }
}

impl wasmtime_wasi::bindings::sync::filesystem::types::HostDescriptor for VfsHostState {
    fn read(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        len: u64,
        offset: u64,
    ) -> Result<
        (Vec<u8>, bool),
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Seek to offset
        self.shared_vfs
            .seek(fd, offset as i64, SEEK_SET)
            .map_err(convert_fs_error_to_trappable)?;

        // Read data
        let mut buf = vec![0u8; len as usize];
        let n = self
            .shared_vfs
            .read(fd, &mut buf)
            .map_err(convert_fs_error_to_trappable)?;

        buf.truncate(n);
        let eof = n < len as usize;
        Ok((buf, eof))
    }

    fn write(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        buffer: Vec<u8>,
        offset: u64,
    ) -> Result<u64, TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Seek to offset
        self.shared_vfs
            .seek(fd, offset as i64, SEEK_SET)
            .map_err(convert_fs_error_to_trappable)?;

        // Write data
        let n = self
            .shared_vfs
            .write(fd, &buffer)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(n as u64)
    }

    fn drop(
        &mut self,
        rep: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<(), anyhow::Error> {
        let wrapper_resource: Resource<FsDescriptorWrapper> = Resource::new_own(rep.rep());
        let wrapper = self.table.delete(wrapper_resource)?;

        // Close the fd in fs-core
        let _ = self.shared_vfs.close(wrapper.fd); // Ignore close errors
        Ok(())
    }

    fn read_via_stream(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        offset: u64,
    ) -> Result<
        Resource<Box<dyn wasmtime_wasi::HostInputStream>>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let wrapper = FsInputStreamWrapper::new(Arc::clone(&self.shared_vfs), fd, offset);
        let resource = self
            .table
            .push(Box::new(wrapper) as Box<dyn HostInputStream>)
            .map_err(TrappableError::trap)?;

        Ok(resource)
    }

    fn write_via_stream(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        offset: u64,
    ) -> Result<
        Resource<Box<dyn wasmtime_wasi::HostOutputStream>>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let wrapper = FsOutputStreamWrapper::new(Arc::clone(&self.shared_vfs), fd, Some(offset));
        let resource = self
            .table
            .push(Box::new(wrapper) as Box<dyn HostOutputStream>)
            .map_err(TrappableError::trap)?;

        Ok(resource)
    }

    fn append_via_stream(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        Resource<Box<dyn wasmtime_wasi::HostOutputStream>>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // None offset means append mode
        let wrapper = FsOutputStreamWrapper::new(Arc::clone(&self.shared_vfs), fd, None);
        let resource = self
            .table
            .push(Box::new(wrapper) as Box<dyn HostOutputStream>)
            .map_err(TrappableError::trap)?;

        Ok(resource)
    }

    fn advise(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _offset: u64,
        _len: u64,
        _advice: wasmtime_wasi::bindings::sync::filesystem::types::Advice,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // Advisory hints: can safely ignore
        Ok(())
    }

    fn sync_data(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // In-memory FS: sync is no-op
        Ok(())
    }

    fn get_flags(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        // Return read+write flags as default
        use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags;
        Ok(DescriptorFlags::READ | DescriptorFlags::WRITE)
    }

    fn get_type(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorType,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let meta = self
            .shared_vfs
            .fstat(fd)
            .map_err(convert_fs_error_to_trappable)?;

        use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorType;
        if meta.is_dir {
            Ok(DescriptorType::Directory)
        } else {
            Ok(DescriptorType::RegularFile)
        }
    }

    fn set_size(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        size: u64,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        self.shared_vfs
            .ftruncate(fd, size)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(())
    }

    fn set_times(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _data_access_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
        _data_modification_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // fs-core doesn't support setting timestamps
        Err(convert_sync_to_nonsync_error(ErrorCode::Unsupported))
    }

    fn set_times_at(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        _path: String,
        _data_access_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
        _data_modification_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // fs-core doesn't support setting timestamps
        Err(convert_sync_to_nonsync_error(ErrorCode::Unsupported))
    }

    fn read_directory(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Get directory entries from fs-core
        let entries = self
            .shared_vfs
            .readdir_fd(fd)
            .map_err(convert_fs_error_to_trappable)?;

        // Create stream wrapper
        let wrapper = FsDirectoryEntryStreamWrapper {
            entries,
            position: 0,
        };
        let wrapper_resource: Resource<FsDirectoryEntryStreamWrapper> = self.table.push(wrapper)?;

        // Transmute to expected return type
        const _: () = {
            use std::mem::{align_of, size_of};
            assert!(
                size_of::<Resource<FsDirectoryEntryStreamWrapper>>()
                    == size_of::<
                        Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
                    >()
            );
            assert!(
                align_of::<Resource<FsDirectoryEntryStreamWrapper>>()
                    == align_of::<
                        Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
                    >()
            );
        };
        let host_stream = unsafe {
            std::mem::transmute::<
                Resource<FsDirectoryEntryStreamWrapper>,
                Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
            >(wrapper_resource)
        };
        Ok(host_stream)
    }

    fn sync(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        Ok(())
    }

    fn create_directory_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let (_, dir_path) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let full_path = self.resolve_path(&dir_path, &path);

        self.shared_vfs
            .mkdir(&full_path)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(())
    }

    fn stat(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let meta = self
            .shared_vfs
            .fstat(fd)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(convert_metadata_to_stat(&meta))
    }

    fn stat_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (_, dir_path) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let full_path = self.resolve_path(&dir_path, &path);

        let meta = self
            .shared_vfs
            .stat(&full_path)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(convert_metadata_to_stat(&meta))
    }

    fn link_at(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _old_path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        _old_path: String,
        _new_descriptor: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _new_path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // fs-core doesn't support hard links
        Err(convert_sync_to_nonsync_error(ErrorCode::Unsupported))
    }

    fn open_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
        open_flags: wasmtime_wasi::bindings::sync::filesystem::types::OpenFlags,
        flags: wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags,
    ) -> Result<
        Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (_, dir_path) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let full_path = self.resolve_path(&dir_path, &path);

        // Convert flags to fs-core flags
        let mut fs_flags = 0u32;

        // Read/write mode
        use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags;
        if flags.contains(DescriptorFlags::READ) && flags.contains(DescriptorFlags::WRITE) {
            fs_flags |= O_RDWR;
        } else if flags.contains(DescriptorFlags::WRITE) {
            fs_flags |= O_WRONLY;
        } else {
            fs_flags |= O_RDONLY;
        }

        // Open flags
        use wasmtime_wasi::bindings::sync::filesystem::types::OpenFlags;
        if open_flags.contains(OpenFlags::CREATE) {
            fs_flags |= O_CREAT;
        }
        if open_flags.contains(OpenFlags::TRUNCATE) {
            fs_flags |= O_TRUNC;
        }

        let fd = self
            .shared_vfs
            .open_path_with_flags(&full_path, fs_flags)
            .map_err(convert_fs_error_to_trappable)?;

        // Check if opened path is a directory
        let is_dir = self.shared_vfs.fstat(fd).map(|m| m.is_dir).unwrap_or(false);

        // Create wrapper
        let wrapper = FsDescriptorWrapper {
            fd,
            path: if is_dir { Some(full_path) } else { None },
        };
        let wrapper_resource: Resource<FsDescriptorWrapper> = self.table.push(wrapper)?;

        // Transmute to expected return type
        const _: () = {
            use std::mem::{align_of, size_of};
            assert!(
                size_of::<Resource<FsDescriptorWrapper>>()
                    == size_of::<Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>>(
                    )
            );
            assert!(
                align_of::<Resource<FsDescriptorWrapper>>()
                    == align_of::<Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>>(
                    )
            );
        };
        let host_descriptor = unsafe {
            std::mem::transmute::<
                Resource<FsDescriptorWrapper>,
                Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
            >(wrapper_resource)
        };
        Ok(host_descriptor)
    }

    fn readlink_at(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _path: String,
    ) -> Result<String, TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // fs-core doesn't support symlinks
        Err(convert_sync_to_nonsync_error(ErrorCode::Unsupported))
    }

    fn remove_directory_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let (_, dir_path) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let full_path = self.resolve_path(&dir_path, &path);

        self.shared_vfs
            .rmdir(&full_path)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(())
    }

    fn rename_at(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _old_path: String,
        _new_descriptor: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _new_path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // fs-core doesn't support rename
        Err(convert_sync_to_nonsync_error(ErrorCode::Unsupported))
    }

    fn symlink_at(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _old_path: String,
        _new_path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // fs-core doesn't support symlinks
        Err(convert_sync_to_nonsync_error(ErrorCode::Unsupported))
    }

    fn unlink_file_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let (_, dir_path) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let full_path = self.resolve_path(&dir_path, &path);

        self.shared_vfs
            .unlink(&full_path)
            .map_err(convert_fs_error_to_trappable)?;

        Ok(())
    }

    fn is_same_object(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        other: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<bool, anyhow::Error> {
        let (fd1, _) = self.get_fs_descriptor(&self_)?;
        let (fd2, _) = self.get_fs_descriptor(&other)?;
        Ok(fd1 == fd2)
    }

    fn metadata_hash(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (fd, _) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let meta = self
            .shared_vfs
            .fstat(fd)
            .map_err(convert_fs_error_to_trappable)?;

        // Simple hash based on size and timestamps
        let lower = meta.size;
        let upper = meta.modified;

        Ok(wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue { lower, upper })
    }

    fn metadata_hash_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let (_, dir_path) = self
            .get_fs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let full_path = self.resolve_path(&dir_path, &path);

        let meta = self
            .shared_vfs
            .stat(&full_path)
            .map_err(convert_fs_error_to_trappable)?;

        let lower = meta.size;
        let upper = meta.modified;

        Ok(wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue { lower, upper })
    }
}

impl wasmtime_wasi::bindings::sync::filesystem::types::HostDirectoryEntryStream for VfsHostState {
    fn read_directory_entry(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
    ) -> Result<
        Option<wasmtime_wasi::bindings::sync::filesystem::types::DirectoryEntry>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let rep = self_.rep();
        let wrapper_resource: Resource<FsDirectoryEntryStreamWrapper> = Resource::new_borrow(rep);

        // Get mutable access to the stream wrapper
        let wrapper = self.table.get_mut(&wrapper_resource).map_err(|e| {
            TrappableError::trap(anyhow::anyhow!(
                "Failed to get directory stream from table: {}",
                e
            ))
        })?;

        if wrapper.position >= wrapper.entries.len() {
            return Ok(None);
        }

        let (name, is_dir) = wrapper.entries[wrapper.position].clone();
        wrapper.position += 1;

        use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorType;
        let type_ = if is_dir {
            DescriptorType::Directory
        } else {
            DescriptorType::RegularFile
        };

        Ok(Some(
            wasmtime_wasi::bindings::sync::filesystem::types::DirectoryEntry { type_, name },
        ))
    }

    fn drop(
        &mut self,
        rep: Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
    ) -> Result<(), anyhow::Error> {
        let wrapper_resource: Resource<FsDirectoryEntryStreamWrapper> =
            Resource::new_own(rep.rep());
        self.table.delete(wrapper_resource)?;
        Ok(())
    }
}

/// Helper to convert sync ErrorCode to non-sync ErrorCode for TrappableError
fn convert_sync_to_nonsync_error(
    sync_error: wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode,
) -> TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode> {
    use wasmtime_wasi::bindings::filesystem::types::ErrorCode as NonSync;
    use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode as Sync;

    let nonsync_error = match sync_error {
        Sync::Access => NonSync::Access,
        Sync::WouldBlock => NonSync::WouldBlock,
        Sync::Already => NonSync::Already,
        Sync::BadDescriptor => NonSync::BadDescriptor,
        Sync::Busy => NonSync::Busy,
        Sync::Deadlock => NonSync::Deadlock,
        Sync::Quota => NonSync::Quota,
        Sync::Exist => NonSync::Exist,
        Sync::FileTooLarge => NonSync::FileTooLarge,
        Sync::IllegalByteSequence => NonSync::IllegalByteSequence,
        Sync::InProgress => NonSync::InProgress,
        Sync::Interrupted => NonSync::Interrupted,
        Sync::Invalid => NonSync::Invalid,
        Sync::Io => NonSync::Io,
        Sync::IsDirectory => NonSync::IsDirectory,
        Sync::Loop => NonSync::Loop,
        Sync::TooManyLinks => NonSync::TooManyLinks,
        Sync::MessageSize => NonSync::MessageSize,
        Sync::NameTooLong => NonSync::NameTooLong,
        Sync::NoDevice => NonSync::NoDevice,
        Sync::NoEntry => NonSync::NoEntry,
        Sync::NoLock => NonSync::NoLock,
        Sync::InsufficientMemory => NonSync::InsufficientMemory,
        Sync::InsufficientSpace => NonSync::InsufficientSpace,
        Sync::NotDirectory => NonSync::NotDirectory,
        Sync::NotEmpty => NonSync::NotEmpty,
        Sync::NotRecoverable => NonSync::NotRecoverable,
        Sync::Unsupported => NonSync::Unsupported,
        Sync::NoTty => NonSync::NoTty,
        Sync::NoSuchDevice => NonSync::NoSuchDevice,
        Sync::Overflow => NonSync::Overflow,
        Sync::NotPermitted => NonSync::NotPermitted,
        Sync::Pipe => NonSync::Pipe,
        Sync::ReadOnly => NonSync::ReadOnly,
        Sync::InvalidSeek => NonSync::InvalidSeek,
        Sync::TextFileBusy => NonSync::TextFileBusy,
        Sync::CrossDevice => NonSync::CrossDevice,
    };

    TrappableError::from(nonsync_error)
}

/// Helper to convert fs-core error to TrappableError
fn convert_fs_error_to_trappable(
    error: fs_core::FsError,
) -> TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode> {
    let sync_error = super::convert_fs_error(error);
    convert_sync_to_nonsync_error(sync_error)
}

/// Helper to convert fs-core Metadata to WASI DescriptorStat
fn convert_metadata_to_stat(
    meta: &fs_core::Metadata,
) -> wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat {
    use wasmtime_wasi::bindings::sync::filesystem::types::{
        Datetime, DescriptorStat, DescriptorType,
    };

    DescriptorStat {
        type_: if meta.is_dir {
            DescriptorType::Directory
        } else {
            DescriptorType::RegularFile
        },
        link_count: 1,
        size: meta.size,
        data_access_timestamp: Some(Datetime {
            seconds: meta.modified,
            nanoseconds: 0,
        }),
        data_modification_timestamp: Some(Datetime {
            seconds: meta.modified,
            nanoseconds: 0,
        }),
        status_change_timestamp: Some(Datetime {
            seconds: meta.created,
            nanoseconds: 0,
        }),
    }
}
