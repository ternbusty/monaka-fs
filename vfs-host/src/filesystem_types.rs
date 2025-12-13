// WASI Filesystem Types Host Implementation
//
// Implements wasi:filesystem/types interface by forwarding to VFS adapter
//
// Phase 1: Empty implementation to discover required methods from compiler

use super::{SharedVfsCore, VfsDescriptorWrapper, VfsDirectoryEntryStreamWrapper, VfsHostState};
use bytes::Bytes;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode;
use wasmtime_wasi::{
    HostInputStream, HostOutputStream, StreamError, StreamResult, Subscribe, TrappableError,
};

/// Wrapper for VFS InputStream that implements HostInputStream
pub struct VfsInputStreamWrapper {
    /// Reference to shared VFS core
    shared_vfs: Arc<Mutex<SharedVfsCore>>,
    /// The VFS InputStream resource
    vfs_stream: crate::exports::wasi::io::streams::InputStream,
}

impl VfsInputStreamWrapper {
    pub fn new(
        shared_vfs: Arc<Mutex<SharedVfsCore>>,
        vfs_stream: crate::exports::wasi::io::streams::InputStream,
    ) -> Self {
        Self {
            shared_vfs,
            vfs_stream,
        }
    }

    /// Lock shared VFS core with proper error handling for poisoned locks
    fn lock_vfs_core(&self) -> Result<std::sync::MutexGuard<'_, SharedVfsCore>, StreamError> {
        self.shared_vfs.lock().map_err(|e| {
            StreamError::LastOperationFailed(anyhow::anyhow!("VFS core lock poisoned: {}", e))
        })
    }
}

/// Wrapper for VFS OutputStream that implements HostOutputStream
pub struct VfsOutputStreamWrapper {
    /// Reference to shared VFS core
    shared_vfs: Arc<Mutex<SharedVfsCore>>,
    /// The VFS OutputStream resource
    vfs_stream: crate::exports::wasi::io::streams::OutputStream,
}

impl VfsOutputStreamWrapper {
    pub fn new(
        shared_vfs: Arc<Mutex<SharedVfsCore>>,
        vfs_stream: crate::exports::wasi::io::streams::OutputStream,
    ) -> Self {
        Self {
            shared_vfs,
            vfs_stream,
        }
    }

    /// Lock shared VFS core with proper error handling for poisoned locks
    fn lock_vfs_core(&self) -> Result<std::sync::MutexGuard<'_, SharedVfsCore>, StreamError> {
        self.shared_vfs.lock().map_err(|e| {
            StreamError::LastOperationFailed(anyhow::anyhow!("VFS core lock poisoned: {}", e))
        })
    }
}

impl wasmtime_wasi::bindings::sync::filesystem::types::Host for VfsHostState {
    fn filesystem_error_code(
        &mut self,
        err: Resource<anyhow::Error>,
    ) -> Result<Option<ErrorCode>, anyhow::Error> {
        // Get the error from the resource table
        let _error = self.table.get(&err)?;

        // Try to downcast to filesystem ErrorCode
        // For now, return None as we don't have specific error mapping
        // In the future, we can examine the error string and map to ErrorCode
        Ok(None)
    }

    fn convert_error_code(
        &mut self,
        err: TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    ) -> Result<wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode, anyhow::Error> {
        // Downcast TrappableError to get the non-sync ErrorCode
        let nonsync_error = err.downcast()?;

        // Convert non-sync to sync ErrorCode
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
    /// Helper: Get VFS descriptor from host descriptor resource.
    /// Retrieves VfsDescriptorWrapper from ResourceTable and returns the inner VFS descriptor.
    fn get_vfs_descriptor(
        &self,
        host_desc: &Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<crate::exports::wasi::filesystem::types::Descriptor, anyhow::Error> {
        // Create a borrow resource with the same rep value but typed as VfsDescriptorWrapper
        let rep = host_desc.rep();
        let wrapper_resource: Resource<VfsDescriptorWrapper> = Resource::new_borrow(rep);

        // Get wrapper from ResourceTable
        let wrapper = self
            .table
            .get(&wrapper_resource)
            .map_err(|e| anyhow::anyhow!("Failed to get descriptor from table: {}", e))?;

        Ok(wrapper.0)
    }
}

/// Helper function to convert VFS StreamError to Host StreamError
fn convert_stream_error(error: crate::exports::wasi::io::streams::StreamError) -> StreamError {
    use crate::exports::wasi::io::streams::StreamError as VfsError;

    match error {
        VfsError::LastOperationFailed(err) => {
            // Convert the error to string since we can't directly map the resource
            StreamError::LastOperationFailed(anyhow::anyhow!("VFS error: {:?}", err))
        }
        VfsError::Closed => StreamError::Closed,
    }
}

#[async_trait::async_trait]
impl Subscribe for VfsInputStreamWrapper {
    async fn ready(&mut self) {
        // For in-memory VFS, streams are always ready
        // No need to wait for I/O
    }
}

impl HostInputStream for VfsInputStreamWrapper {
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        // Lock VFS core and call VFS stream read
        let core = self.lock_vfs_core()?;
        let mut vfs_store =
            lock_vfs_store(&core.vfs_store).map_err(StreamError::LastOperationFailed)?;

        // Call VFS InputStream read method
        let result = core
            .vfs_instance
            .wasi_io_streams()
            .input_stream()
            .call_read(&mut *vfs_store, self.vfs_stream, size as u64);

        match result {
            Ok(Ok(data)) => Ok(Bytes::from(data)),
            Ok(Err(err)) => Err(convert_stream_error(err)),
            Err(e) => Err(StreamError::LastOperationFailed(e)),
        }
    }

    fn skip(&mut self, nelem: usize) -> StreamResult<usize> {
        // Lock VFS core and call VFS stream skip
        let core = self.lock_vfs_core()?;
        let mut vfs_store =
            lock_vfs_store(&core.vfs_store).map_err(StreamError::LastOperationFailed)?;

        // Call VFS InputStream skip method
        let result = core
            .vfs_instance
            .wasi_io_streams()
            .input_stream()
            .call_skip(&mut *vfs_store, self.vfs_stream, nelem as u64);

        match result {
            Ok(Ok(skipped)) => {
                let skipped_usize = skipped.try_into().map_err(|_| {
                    StreamError::LastOperationFailed(anyhow::anyhow!(
                        "Skipped size {} exceeds usize::MAX",
                        skipped
                    ))
                })?;
                Ok(skipped_usize)
            }
            Ok(Err(err)) => Err(convert_stream_error(err)),
            Err(e) => Err(StreamError::LastOperationFailed(e)),
        }
    }
}

#[async_trait::async_trait]
impl Subscribe for VfsOutputStreamWrapper {
    async fn ready(&mut self) {
        // For in-memory VFS, streams are always ready
        // No need to wait for I/O
    }
}

impl HostOutputStream for VfsOutputStreamWrapper {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        // Lock VFS core and call VFS stream write
        let core = self.lock_vfs_core()?;
        let mut vfs_store =
            lock_vfs_store(&core.vfs_store).map_err(StreamError::LastOperationFailed)?;

        // Call VFS OutputStream write method
        let result = core
            .vfs_instance
            .wasi_io_streams()
            .output_stream()
            .call_write(&mut *vfs_store, self.vfs_stream, &bytes);

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(convert_stream_error(err)),
            Err(e) => Err(StreamError::LastOperationFailed(e)),
        }
    }

    fn flush(&mut self) -> StreamResult<()> {
        // Lock VFS core and call VFS stream flush
        let core = self.lock_vfs_core()?;
        let mut vfs_store =
            lock_vfs_store(&core.vfs_store).map_err(StreamError::LastOperationFailed)?;

        // Call VFS OutputStream flush method
        let result = core
            .vfs_instance
            .wasi_io_streams()
            .output_stream()
            .call_flush(&mut *vfs_store, self.vfs_stream);

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(convert_stream_error(err)),
            Err(e) => Err(StreamError::LastOperationFailed(e)),
        }
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        // Lock VFS core and call VFS stream check_write
        let core = self.lock_vfs_core()?;
        let mut vfs_store =
            lock_vfs_store(&core.vfs_store).map_err(StreamError::LastOperationFailed)?;

        // Call VFS OutputStream check_write method
        let result = core
            .vfs_instance
            .wasi_io_streams()
            .output_stream()
            .call_check_write(&mut *vfs_store, self.vfs_stream);

        match result {
            Ok(Ok(size)) => {
                let size_usize = size.try_into().map_err(|_| {
                    StreamError::LastOperationFailed(anyhow::anyhow!(
                        "Size {} exceeds usize::MAX",
                        size
                    ))
                })?;
                Ok(size_usize)
            }
            Ok(Err(err)) => Err(convert_stream_error(err)),
            Err(e) => Err(StreamError::LastOperationFailed(e)),
        }
    }
}

// Helper methods for VfsHostState to handle lock poisoning
impl VfsHostState {
    /// Lock shared VFS core with proper error handling for poisoned locks
    pub(crate) fn lock_vfs_core(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, SharedVfsCore>, anyhow::Error> {
        self.shared_vfs
            .lock()
            .map_err(|e| anyhow::anyhow!("VFS core lock poisoned: {}", e))
    }
}

// Helper function for locking vfs_store
fn lock_vfs_store(
    arc_store: &Arc<Mutex<wasmtime::Store<crate::VfsStoreData>>>,
) -> Result<std::sync::MutexGuard<'_, wasmtime::Store<crate::VfsStoreData>>, anyhow::Error> {
    arc_store
        .lock()
        .map_err(|e| anyhow::anyhow!("VFS store lock poisoned: {}", e))
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
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Lock shared VFS core
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        // Call VFS adapter's read method
        // Note: WASI descriptor.read signature is (length, offset) not (offset, length)
        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_read(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                len,
                offset,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok((data, end)) => Ok((data, end)),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn write(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        buffer: Vec<u8>,
        offset: u64,
    ) -> Result<u64, TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Lock shared VFS core
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_write(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &buffer,
                offset,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(written) => Ok(written),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn drop(
        &mut self,
        rep: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<(), anyhow::Error> {
        // Delete from ResourceTable using wrapper type
        let wrapper_resource: Resource<VfsDescriptorWrapper> = Resource::new_own(rep.rep());
        self.table.delete(wrapper_resource)?;
        Ok(())
    }

    // Stubs for remaining methods - will implement next
    fn read_via_stream(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        offset: u64,
    ) -> Result<
        Resource<Box<dyn wasmtime_wasi::HostInputStream>>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Lock shared VFS core and call VFS adapter's read_via_stream
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = lock_vfs_store(&vfs_store_arc).map_err(TrappableError::trap)?;
            core.vfs_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_read_via_stream(&mut *vfs_store, vfs_desc, offset)
                .map_err(TrappableError::trap)?
        };

        match result {
            Ok(vfs_stream) => {
                // Create wrapper for the VFS stream
                let wrapper = VfsInputStreamWrapper::new(Arc::clone(&self.shared_vfs), vfs_stream);

                // Add to resource table
                let resource = self
                    .table
                    .push(Box::new(wrapper) as Box<dyn HostInputStream>)
                    .map_err(TrappableError::trap)?;

                Ok(resource)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn write_via_stream(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        offset: u64,
    ) -> Result<
        Resource<Box<dyn wasmtime_wasi::HostOutputStream>>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Lock shared VFS core and call VFS adapter's write_via_stream
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = lock_vfs_store(&vfs_store_arc).map_err(TrappableError::trap)?;
            core.vfs_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_write_via_stream(&mut *vfs_store, vfs_desc, offset)
                .map_err(TrappableError::trap)?
        };

        match result {
            Ok(vfs_stream) => {
                // Create wrapper for the VFS stream
                let wrapper = VfsOutputStreamWrapper::new(Arc::clone(&self.shared_vfs), vfs_stream);

                // Add to resource table
                let resource = self
                    .table
                    .push(Box::new(wrapper) as Box<dyn HostOutputStream>)
                    .map_err(TrappableError::trap)?;

                Ok(resource)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn append_via_stream(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        Resource<Box<dyn wasmtime_wasi::HostOutputStream>>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Lock shared VFS core and call VFS adapter's append_via_stream
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = lock_vfs_store(&vfs_store_arc).map_err(TrappableError::trap)?;
            core.vfs_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_append_via_stream(&mut *vfs_store, vfs_desc)
                .map_err(TrappableError::trap)?
        };

        match result {
            Ok(vfs_stream) => {
                // Create wrapper for the VFS stream
                let wrapper = VfsOutputStreamWrapper::new(Arc::clone(&self.shared_vfs), vfs_stream);

                // Add to resource table
                let resource = self
                    .table
                    .push(Box::new(wrapper) as Box<dyn HostOutputStream>)
                    .map_err(TrappableError::trap)?;

                Ok(resource)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn advise(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        _offset: u64,
        _len: u64,
        _advice: wasmtime_wasi::bindings::sync::filesystem::types::Advice,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // Advisory hints - can safely ignore
        Ok(())
    }

    fn sync_data(
        &mut self,
        _self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        // In-memory FS - sync is no-op
        Ok(())
    }

    fn get_flags(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_get_flags(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(vfs_flags) => {
                // Convert VFS flags to host flags
                let host_flags = convert_descriptor_flags_from_vfs(vfs_flags);
                Ok(host_flags)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn get_type(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorType,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_get_type(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(vfs_type) => {
                // Convert VFS type to host type
                let host_type = convert_descriptor_type(vfs_type);
                Ok(host_type)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn set_size(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        size: u64,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_set_size(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                size,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn set_times(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        data_access_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
        data_modification_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let vfs_atime = convert_new_timestamp_to_vfs(data_access_timestamp);
        let vfs_mtime = convert_new_timestamp_to_vfs(data_modification_timestamp);

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_set_times(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                vfs_atime,
                vfs_mtime,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn set_times_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
        data_access_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
        data_modification_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let vfs_path_flags = convert_path_flags_to_vfs(path_flags);
        let vfs_atime = convert_new_timestamp_to_vfs(data_access_timestamp);
        let vfs_mtime = convert_new_timestamp_to_vfs(data_modification_timestamp);

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_set_times_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                vfs_path_flags,
                &path,
                vfs_atime,
                vfs_mtime,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn read_directory(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Call VFS adapter's read_directory
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = lock_vfs_store(&vfs_store_arc).map_err(TrappableError::trap)?;
            core.vfs_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_read_directory(&mut *vfs_store, vfs_desc)
                .map_err(TrappableError::trap)?
        };

        match result {
            Ok(vfs_stream) => {
                // Create host directory entry stream with VfsDirectoryEntryStreamWrapper
                let wrapper = VfsDirectoryEntryStreamWrapper(vfs_stream);
                let wrapper_resource: Resource<VfsDirectoryEntryStreamWrapper> =
                    self.table.push(wrapper)?;

                // Transmute to expected return type
                // SAFETY: Resource<T> is a transparent u32 wrapper
                const _: () = {
                    use std::mem::{align_of, size_of};
                    assert!(
                        size_of::<Resource<VfsDirectoryEntryStreamWrapper>>()
                            == size_of::<Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>>()
                    );
                    assert!(
                        align_of::<Resource<VfsDirectoryEntryStreamWrapper>>()
                            == align_of::<Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>>()
                    );
                };
                let host_stream = unsafe {
                    std::mem::transmute::<
                        Resource<VfsDirectoryEntryStreamWrapper>,
                        Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
                    >(wrapper_resource)
                };
                Ok(host_stream)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
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
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_create_directory_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn stat(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Call VFS adapter's stat
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_stat(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(vfs_stat) => Ok(convert_descriptor_stat_from_vfs(vfs_stat)),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn stat_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let vfs_path_flags = convert_path_flags_to_vfs(path_flags);

        // Call VFS adapter's stat_at
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_stat_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                vfs_path_flags,
                &path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(vfs_stat) => Ok(convert_descriptor_stat_from_vfs(vfs_stat)),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn link_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        old_path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        old_path: String,
        new_descriptor: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        new_path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let vfs_new_desc = self
            .get_vfs_descriptor(&new_descriptor)
            .map_err(TrappableError::trap)?;
        let vfs_path_flags = convert_path_flags_to_vfs(old_path_flags);

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_link_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                vfs_path_flags,
                &old_path,
                vfs_new_desc,
                &new_path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn open_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
        open_flags: wasmtime_wasi::bindings::sync::filesystem::types::OpenFlags,
        flags: wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags,
    ) -> Result<
        Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Convert host types to VFS types
        let vfs_path_flags = convert_path_flags_to_vfs(path_flags);
        let vfs_open_flags = convert_open_flags_to_vfs(open_flags);
        let vfs_flags = convert_descriptor_flags_to_vfs(flags);

        // Call VFS adapter's open_at
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let vfs_store_arc = core.vfs_store.clone();
            let mut vfs_store = lock_vfs_store(&vfs_store_arc).map_err(TrappableError::trap)?;
            core.vfs_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_open_at(
                    &mut *vfs_store,
                    vfs_desc,
                    vfs_path_flags,
                    &path,
                    vfs_open_flags,
                    vfs_flags,
                )
                .map_err(TrappableError::trap)?
        };

        match result {
            Ok(vfs_new_desc) => {
                // Create host descriptor with VfsDescriptorWrapper
                let wrapper = VfsDescriptorWrapper(vfs_new_desc);
                let wrapper_resource: Resource<VfsDescriptorWrapper> = self.table.push(wrapper)?;

                // Transmute to expected return type
                // SAFETY: Resource<T> is a transparent u32 wrapper
                const _: () = {
                    use std::mem::{align_of, size_of};
                    assert!(
                        size_of::<Resource<VfsDescriptorWrapper>>()
                            == size_of::<
                                Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                            >()
                    );
                    assert!(
                        align_of::<Resource<VfsDescriptorWrapper>>()
                            == align_of::<
                                Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                            >()
                    );
                };
                let host_descriptor = unsafe {
                    std::mem::transmute::<
                        Resource<VfsDescriptorWrapper>,
                        Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                    >(wrapper_resource)
                };
                Ok(host_descriptor)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn readlink_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path: String,
    ) -> Result<String, TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_readlink_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(link_path) => Ok(link_path),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn remove_directory_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_remove_directory_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn rename_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        old_path: String,
        new_descriptor: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        new_path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;
        let vfs_new_desc = self
            .get_vfs_descriptor(&new_descriptor)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_rename_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &old_path,
                vfs_new_desc,
                &new_path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn symlink_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        old_path: String,
        new_path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_symlink_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &old_path,
                &new_path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn unlink_file_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path: String,
    ) -> Result<(), TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>> {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_unlink_file_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                &path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(()) => Ok(()),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn is_same_object(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        other: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<bool, anyhow::Error> {
        let vfs_desc = self.get_vfs_descriptor(&self_)?;
        let vfs_other = self.get_vfs_descriptor(&other)?;

        let core = self.lock_vfs_core()?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_is_same_object(&mut *lock_vfs_store(&core.vfs_store)?, vfs_desc, vfs_other)?;

        Ok(result)
    }

    fn metadata_hash(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_metadata_hash(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(vfs_hash) => {
                // Convert VFS MetadataHashValue to host MetadataHashValue
                let host_hash =
                    wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue {
                        lower: vfs_hash.lower,
                        upper: vfs_hash.upper,
                    };
                Ok(host_hash)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn metadata_hash_at(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
        path_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
        path: String,
    ) -> Result<
        wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        let vfs_path_flags = convert_path_flags_to_vfs(path_flags);

        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_metadata_hash_at(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_desc,
                vfs_path_flags,
                &path,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(vfs_hash) => {
                // Convert VFS MetadataHashValue to host MetadataHashValue
                let host_hash =
                    wasmtime_wasi::bindings::sync::filesystem::types::MetadataHashValue {
                        lower: vfs_hash.lower,
                        upper: vfs_hash.upper,
                    };
                Ok(host_hash)
            }
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
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
        // Get VFS stream from ResourceTable using wrapper type
        let rep = self_.rep();
        let wrapper_resource: Resource<VfsDirectoryEntryStreamWrapper> = Resource::new_borrow(rep);
        let vfs_stream = self
            .table
            .get(&wrapper_resource)
            .map_err(|e| {
                TrappableError::trap(anyhow::anyhow!(
                    "Failed to get directory stream from table: {}",
                    e
                ))
            })?
            .0;

        // Lock shared VFS core
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        // Call VFS adapter's read_directory_entry
        let result = core
            .vfs_instance
            .wasi_filesystem_types()
            .directory_entry_stream()
            .call_read_directory_entry(
                &mut *lock_vfs_store(&core.vfs_store).map_err(TrappableError::trap)?,
                vfs_stream,
            )
            .map_err(TrappableError::trap)?;

        match result {
            Ok(Some(vfs_entry)) => {
                // Convert VFS DirectoryEntry to host DirectoryEntry
                Ok(Some(
                    wasmtime_wasi::bindings::sync::filesystem::types::DirectoryEntry {
                        type_: convert_descriptor_type(vfs_entry.type_),
                        name: vfs_entry.name,
                    },
                ))
            }
            Ok(None) => Ok(None),
            Err(vfs_error) => {
                let host_error = super::convert_vfs_error(vfs_error);
                Err(convert_sync_to_nonsync_error(host_error))
            }
        }
    }

    fn drop(
        &mut self,
        rep: Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
    ) -> Result<(), anyhow::Error> {
        // Delete from ResourceTable using wrapper type
        let wrapper_resource: Resource<VfsDirectoryEntryStreamWrapper> =
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

/// Helper to convert VFS DescriptorType to host DescriptorType
fn convert_descriptor_type(
    vfs_type: crate::exports::wasi::filesystem::types::DescriptorType,
) -> wasmtime_wasi::bindings::sync::filesystem::types::DescriptorType {
    use crate::exports::wasi::filesystem::types::DescriptorType as VfsType;
    use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorType as HostType;

    match vfs_type {
        VfsType::Unknown => HostType::Unknown,
        VfsType::BlockDevice => HostType::BlockDevice,
        VfsType::CharacterDevice => HostType::CharacterDevice,
        VfsType::Directory => HostType::Directory,
        VfsType::Fifo => HostType::Fifo,
        VfsType::SymbolicLink => HostType::SymbolicLink,
        VfsType::RegularFile => HostType::RegularFile,
        VfsType::Socket => HostType::Socket,
    }
}

/// Helper to convert host PathFlags to VFS PathFlags
fn convert_path_flags_to_vfs(
    host_flags: wasmtime_wasi::bindings::sync::filesystem::types::PathFlags,
) -> crate::exports::wasi::filesystem::types::PathFlags {
    use crate::exports::wasi::filesystem::types::PathFlags as VfsFlags;
    use wasmtime_wasi::bindings::sync::filesystem::types::PathFlags as HostFlags;

    let mut vfs_flags = VfsFlags::empty();
    if host_flags.contains(HostFlags::SYMLINK_FOLLOW) {
        vfs_flags |= VfsFlags::SYMLINK_FOLLOW;
    }
    vfs_flags
}

/// Helper to convert host OpenFlags to VFS OpenFlags
fn convert_open_flags_to_vfs(
    host_flags: wasmtime_wasi::bindings::sync::filesystem::types::OpenFlags,
) -> crate::exports::wasi::filesystem::types::OpenFlags {
    use crate::exports::wasi::filesystem::types::OpenFlags as VfsFlags;
    use wasmtime_wasi::bindings::sync::filesystem::types::OpenFlags as HostFlags;

    let mut vfs_flags = VfsFlags::empty();
    if host_flags.contains(HostFlags::CREATE) {
        vfs_flags |= VfsFlags::CREATE;
    }
    if host_flags.contains(HostFlags::DIRECTORY) {
        vfs_flags |= VfsFlags::DIRECTORY;
    }
    if host_flags.contains(HostFlags::EXCLUSIVE) {
        vfs_flags |= VfsFlags::EXCLUSIVE;
    }
    if host_flags.contains(HostFlags::TRUNCATE) {
        vfs_flags |= VfsFlags::TRUNCATE;
    }
    vfs_flags
}

/// Helper to convert host DescriptorFlags to VFS DescriptorFlags
fn convert_descriptor_flags_to_vfs(
    host_flags: wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags,
) -> crate::exports::wasi::filesystem::types::DescriptorFlags {
    use crate::exports::wasi::filesystem::types::DescriptorFlags as VfsFlags;
    use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags as HostFlags;

    let mut vfs_flags = VfsFlags::empty();
    if host_flags.contains(HostFlags::READ) {
        vfs_flags |= VfsFlags::READ;
    }
    if host_flags.contains(HostFlags::WRITE) {
        vfs_flags |= VfsFlags::WRITE;
    }
    if host_flags.contains(HostFlags::FILE_INTEGRITY_SYNC) {
        vfs_flags |= VfsFlags::FILE_INTEGRITY_SYNC;
    }
    if host_flags.contains(HostFlags::DATA_INTEGRITY_SYNC) {
        vfs_flags |= VfsFlags::DATA_INTEGRITY_SYNC;
    }
    if host_flags.contains(HostFlags::REQUESTED_WRITE_SYNC) {
        vfs_flags |= VfsFlags::REQUESTED_WRITE_SYNC;
    }
    if host_flags.contains(HostFlags::MUTATE_DIRECTORY) {
        vfs_flags |= VfsFlags::MUTATE_DIRECTORY;
    }
    vfs_flags
}

/// Helper to convert VFS DescriptorFlags to host DescriptorFlags
fn convert_descriptor_flags_from_vfs(
    vfs_flags: crate::exports::wasi::filesystem::types::DescriptorFlags,
) -> wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags {
    use crate::exports::wasi::filesystem::types::DescriptorFlags as VfsFlags;
    use wasmtime_wasi::bindings::sync::filesystem::types::DescriptorFlags as HostFlags;

    let mut host_flags = HostFlags::empty();
    if vfs_flags.contains(VfsFlags::READ) {
        host_flags |= HostFlags::READ;
    }
    if vfs_flags.contains(VfsFlags::WRITE) {
        host_flags |= HostFlags::WRITE;
    }
    if vfs_flags.contains(VfsFlags::FILE_INTEGRITY_SYNC) {
        host_flags |= HostFlags::FILE_INTEGRITY_SYNC;
    }
    if vfs_flags.contains(VfsFlags::DATA_INTEGRITY_SYNC) {
        host_flags |= HostFlags::DATA_INTEGRITY_SYNC;
    }
    if vfs_flags.contains(VfsFlags::REQUESTED_WRITE_SYNC) {
        host_flags |= HostFlags::REQUESTED_WRITE_SYNC;
    }
    if vfs_flags.contains(VfsFlags::MUTATE_DIRECTORY) {
        host_flags |= HostFlags::MUTATE_DIRECTORY;
    }
    host_flags
}

/// Helper to convert VFS DescriptorStat to host DescriptorStat
fn convert_descriptor_stat_from_vfs(
    vfs_stat: crate::exports::wasi::filesystem::types::DescriptorStat,
) -> wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat {
    wasmtime_wasi::bindings::sync::filesystem::types::DescriptorStat {
        type_: convert_descriptor_type(vfs_stat.type_),
        link_count: vfs_stat.link_count,
        size: vfs_stat.size,
        data_access_timestamp: vfs_stat.data_access_timestamp.map(|t| {
            wasmtime_wasi::bindings::sync::filesystem::types::Datetime {
                seconds: t.seconds,
                nanoseconds: t.nanoseconds,
            }
        }),
        data_modification_timestamp: vfs_stat.data_modification_timestamp.map(|t| {
            wasmtime_wasi::bindings::sync::filesystem::types::Datetime {
                seconds: t.seconds,
                nanoseconds: t.nanoseconds,
            }
        }),
        status_change_timestamp: vfs_stat.status_change_timestamp.map(|t| {
            wasmtime_wasi::bindings::sync::filesystem::types::Datetime {
                seconds: t.seconds,
                nanoseconds: t.nanoseconds,
            }
        }),
    }
}

/// Helper to convert host NewTimestamp to VFS NewTimestamp
fn convert_new_timestamp_to_vfs(
    host_timestamp: wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp,
) -> crate::exports::wasi::filesystem::types::NewTimestamp {
    use crate::exports::wasi::filesystem::types::NewTimestamp as VfsNewTimestamp;
    use wasmtime_wasi::bindings::sync::filesystem::types::NewTimestamp as HostNewTimestamp;

    match host_timestamp {
        HostNewTimestamp::NoChange => VfsNewTimestamp::NoChange,
        HostNewTimestamp::Now => VfsNewTimestamp::Now,
        HostNewTimestamp::Timestamp(dt) => {
            VfsNewTimestamp::Timestamp(crate::exports::wasi::filesystem::types::Datetime {
                seconds: dt.seconds,
                nanoseconds: dt.nanoseconds,
            })
        }
    }
}
