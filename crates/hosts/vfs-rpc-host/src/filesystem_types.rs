// WASI Filesystem Types Host Implementation
//
// Implements wasi:filesystem/types interface by forwarding to RPC adapter
//
// Phase 1: Empty implementation to discover required methods from compiler

use super::{
    RpcDescriptorWrapper, RpcDirectoryEntryStreamWrapper, SharedRpcAdapterCore, VfsRpcHostState,
};
use bytes::Bytes;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode;
use wasmtime_wasi::{
    HostInputStream, HostOutputStream, StreamError, StreamResult, Subscribe, TrappableError,
};

/// Wrapper for VFS InputStream that implements HostInputStream
/// Uses Descriptor::read directly instead of going through wasi:io/streams
/// to avoid nested runtime issues (WASI exports cannot call WASI imports).
pub struct VfsInputStreamWrapper {
    /// Reference to shared VFS core
    shared_rpc: Arc<Mutex<SharedRpcAdapterCore>>,
    /// The VFS Descriptor resource (file handle)
    descriptor: crate::exports::wasi::filesystem::types::Descriptor,
    /// Current read offset
    offset: std::cell::Cell<u64>,
}

impl VfsInputStreamWrapper {
    pub fn new(
        shared_rpc: Arc<Mutex<SharedRpcAdapterCore>>,
        descriptor: crate::exports::wasi::filesystem::types::Descriptor,
        offset: u64,
    ) -> Self {
        Self {
            shared_rpc,
            descriptor,
            offset: std::cell::Cell::new(offset),
        }
    }
}

/// Wrapper for VFS OutputStream that implements HostOutputStream
/// Uses Descriptor::write directly instead of going through wasi:io/streams
/// to avoid nested runtime issues (WASI exports cannot call WASI imports).
pub struct VfsOutputStreamWrapper {
    /// Reference to shared VFS core
    shared_rpc: Arc<Mutex<SharedRpcAdapterCore>>,
    /// The VFS Descriptor resource (file handle)
    descriptor: crate::exports::wasi::filesystem::types::Descriptor,
    /// Current write offset
    offset: std::cell::Cell<u64>,
}

impl VfsOutputStreamWrapper {
    pub fn new(
        shared_rpc: Arc<Mutex<SharedRpcAdapterCore>>,
        descriptor: crate::exports::wasi::filesystem::types::Descriptor,
        offset: u64,
    ) -> Self {
        Self {
            shared_rpc,
            descriptor,
            offset: std::cell::Cell::new(offset),
        }
    }
}

impl wasmtime_wasi::bindings::sync::filesystem::types::Host for VfsRpcHostState {
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

impl VfsRpcHostState {
    /// Helper: Get VFS descriptor from host descriptor resource
    fn get_vfs_descriptor(
        &self,
        host_desc: &Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
    ) -> Result<crate::exports::wasi::filesystem::types::Descriptor, anyhow::Error> {
        // Get descriptor from ResourceTable using wrapper type
        let rep = host_desc.rep();
        let wrapper_resource: Resource<RpcDescriptorWrapper> = Resource::new_borrow(rep);
        let wrapper = self
            .table
            .get(&wrapper_resource)
            .map_err(|e| anyhow::anyhow!("Failed to get descriptor from table: {}", e))?;
        Ok(wrapper.0)
    }
}

/// Helper function to convert VFS filesystem ErrorCode to StreamError
fn convert_fs_error_to_stream(
    error: crate::exports::wasi::filesystem::types::ErrorCode,
) -> StreamError {
    StreamError::LastOperationFailed(anyhow::anyhow!("Filesystem error: {:?}", error))
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
        let shared_rpc = &self.shared_rpc;
        let descriptor = self.descriptor;
        let current_offset = self.offset.get();

        // Run RPC operation in a separate thread to avoid tokio runtime nesting.
        // When this method is called from wasmtime-wasi's blocking_read_and_flush,
        // we're already inside a tokio runtime. Calling into the RPC adapter
        // (which also uses wasmtime-wasi for sockets) would create a nested runtime.
        // Using std::thread::scope allows the RPC call to run in a fresh thread
        // without the parent's tokio context.
        let result = std::thread::scope(|s| {
            s.spawn(|| {
                let core = shared_rpc
                    .lock()
                    .map_err(|e| anyhow::anyhow!("VFS core lock poisoned: {}", e))?;
                let rpc_store_arc = core.rpc_store.clone();
                let mut rpc_store = rpc_store_arc
                    .lock()
                    .map_err(|e| anyhow::anyhow!("RPC store lock poisoned: {}", e))?;

                core.rpc_instance
                    .wasi_filesystem_types()
                    .descriptor()
                    .call_read(&mut *rpc_store, descriptor, size as u64, current_offset)
            })
            .join()
        });

        // Process the nested Result types:
        // Result<Result<Result<(Vec<u8>, bool), ErrorCode>, anyhow::Error>, JoinError>
        match result {
            Ok(Ok(Ok((data, _end_of_stream)))) => {
                self.offset.set(current_offset + data.len() as u64);
                Ok(Bytes::from(data))
            }
            Ok(Ok(Err(err))) => Err(convert_fs_error_to_stream(err)),
            Ok(Err(e)) => Err(StreamError::LastOperationFailed(e)),
            Err(_) => Err(StreamError::LastOperationFailed(anyhow::anyhow!(
                "RPC thread panicked"
            ))),
        }
    }

    fn skip(&mut self, nelem: usize) -> StreamResult<usize> {
        // Simply advance the offset without reading
        let current_offset = self.offset.get();
        self.offset.set(current_offset + nelem as u64);
        Ok(nelem)
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
        let shared_rpc = &self.shared_rpc;
        let descriptor = self.descriptor;
        let current_offset = self.offset.get();
        // Clone bytes for the thread (Bytes is cheap to clone - reference counted)
        let bytes_clone = bytes.clone();

        // Run RPC operation in a separate thread to avoid tokio runtime nesting.
        // When this method is called from wasmtime-wasi's blocking_write_and_flush,
        // we're already inside a tokio runtime. Calling into the RPC adapter
        // (which also uses wasmtime-wasi for sockets) would create a nested runtime.
        // Using std::thread::scope allows the RPC call to run in a fresh thread
        // without the parent's tokio context.
        let result = std::thread::scope(|s| {
            s.spawn(|| {
                let core = shared_rpc
                    .lock()
                    .map_err(|e| anyhow::anyhow!("VFS core lock poisoned: {}", e))?;
                let rpc_store_arc = core.rpc_store.clone();
                let mut rpc_store = rpc_store_arc
                    .lock()
                    .map_err(|e| anyhow::anyhow!("RPC store lock poisoned: {}", e))?;

                core.rpc_instance
                    .wasi_filesystem_types()
                    .descriptor()
                    .call_write(&mut *rpc_store, descriptor, &bytes_clone, current_offset)
            })
            .join()
        });

        // Process the nested Result types:
        // Result<Result<Result<u64, ErrorCode>, anyhow::Error>, JoinError>
        match result {
            Ok(Ok(Ok(written))) => {
                self.offset.set(current_offset + written);
                Ok(())
            }
            Ok(Ok(Err(err))) => Err(convert_fs_error_to_stream(err)),
            Ok(Err(e)) => Err(StreamError::LastOperationFailed(e)),
            Err(_) => Err(StreamError::LastOperationFailed(anyhow::anyhow!(
                "RPC thread panicked"
            ))),
        }
    }

    fn flush(&mut self) -> StreamResult<()> {
        // For filesystem writes via Descriptor::write, data is immediately written
        // No separate flush needed
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        // Always ready to accept writes up to 64KB
        Ok(65536)
    }
}

// Helper methods for VfsRpcHostState to handle lock poisoning
impl VfsRpcHostState {
    /// Lock shared VFS core with proper error handling for poisoned locks
    pub(crate) fn lock_vfs_core(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, SharedRpcAdapterCore>, anyhow::Error> {
        self.shared_rpc
            .lock()
            .map_err(|e| anyhow::anyhow!("VFS core lock poisoned: {}", e))
    }
}

// Helper function for locking rpc_store
fn lock_rpc_store(
    arc_store: &Arc<Mutex<wasmtime::Store<crate::RpcStoreData>>>,
) -> Result<std::sync::MutexGuard<'_, wasmtime::Store<crate::RpcStoreData>>, anyhow::Error> {
    arc_store
        .lock()
        .map_err(|e| anyhow::anyhow!("VFS store lock poisoned: {}", e))
}

impl wasmtime_wasi::bindings::sync::filesystem::types::HostDescriptor for VfsRpcHostState {
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

        // Call RPC adapter's read method
        // Note: WASI descriptor.read signature is (length, offset) not (offset, length)
        let result = core
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_read(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_write(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
        let wrapper_resource: Resource<RpcDescriptorWrapper> = Resource::new_own(rep.rep());
        self.table.delete(wrapper_resource)?;
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
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Create wrapper directly with descriptor + offset
        // (bypasses rpc-adapter's wasi:io/streams to avoid nested runtime issues)
        let wrapper = VfsInputStreamWrapper::new(Arc::clone(&self.shared_rpc), vfs_desc, offset);

        // Add to resource table
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
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Create wrapper directly with descriptor + offset
        // (bypasses rpc-adapter's wasi:io/streams to avoid nested runtime issues)
        let wrapper = VfsOutputStreamWrapper::new(Arc::clone(&self.shared_rpc), vfs_desc, offset);

        // Add to resource table
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
        // Get VFS descriptor
        let vfs_desc = self
            .get_vfs_descriptor(&self_)
            .map_err(TrappableError::trap)?;

        // Get file size to determine append offset
        let file_size = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let rpc_store_arc = core.rpc_store.clone();
            let mut rpc_store = lock_rpc_store(&rpc_store_arc).map_err(TrappableError::trap)?;
            let result = core
                .rpc_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_stat(&mut *rpc_store, vfs_desc)
                .map_err(TrappableError::trap)?;
            match result {
                Ok(stat) => stat.size,
                Err(vfs_error) => {
                    let host_error = super::convert_vfs_error(vfs_error);
                    return Err(convert_sync_to_nonsync_error(host_error));
                }
            }
        };

        // Create wrapper directly with descriptor + file size as offset
        // (bypasses rpc-adapter's wasi:io/streams to avoid nested runtime issues)
        let wrapper =
            VfsOutputStreamWrapper::new(Arc::clone(&self.shared_rpc), vfs_desc, file_size);

        // Add to resource table
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_get_flags(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_get_type(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_set_size(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_set_times(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_set_times_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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

        // Call RPC adapter's read_directory
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let rpc_store_arc = core.rpc_store.clone();
            let mut rpc_store = lock_rpc_store(&rpc_store_arc).map_err(TrappableError::trap)?;
            core.rpc_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_read_directory(&mut *rpc_store, vfs_desc)
                .map_err(TrappableError::trap)?
        };

        match result {
            Ok(vfs_stream) => {
                // Push RpcDirectoryEntryStreamWrapper to ResourceTable (proper typed storage)
                let wrapper = RpcDirectoryEntryStreamWrapper(vfs_stream);
                let wrapper_resource: Resource<RpcDirectoryEntryStreamWrapper> =
                    self.table.push(wrapper)?;

                // Transmute Resource<RpcDirectoryEntryStreamWrapper> to Resource<DirectoryEntryStream>
                // SAFETY: Resource<T> is a transparent u32 wrapper, so transmute is safe
                const _: () = {
                    use std::mem::{align_of, size_of};
                    assert!(
                        size_of::<Resource<RpcDirectoryEntryStreamWrapper>>()
                            == size_of::<Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>>()
                    );
                    assert!(
                        align_of::<Resource<RpcDirectoryEntryStreamWrapper>>()
                            == align_of::<Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>>()
                    );
                };
                let host_stream = unsafe {
                    std::mem::transmute::<
                        Resource<RpcDirectoryEntryStreamWrapper>,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_create_directory_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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

        // Call RPC adapter's stat
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_stat(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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

        // Call RPC adapter's stat_at
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_stat_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_link_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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

        // Call RPC adapter's open_at
        let result = {
            let core = self.lock_vfs_core().map_err(TrappableError::trap)?;
            let rpc_store_arc = core.rpc_store.clone();
            let mut rpc_store = lock_rpc_store(&rpc_store_arc).map_err(TrappableError::trap)?;
            core.rpc_instance
                .wasi_filesystem_types()
                .descriptor()
                .call_open_at(
                    &mut *rpc_store,
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
                // Push RpcDescriptorWrapper to ResourceTable (proper typed storage)
                let wrapper = RpcDescriptorWrapper(vfs_new_desc);
                let wrapper_resource: Resource<RpcDescriptorWrapper> = self.table.push(wrapper)?;

                // Transmute Resource<RpcDescriptorWrapper> to Resource<Descriptor>
                // SAFETY: Resource<T> is a transparent u32 wrapper, so transmute is safe
                const _: () = {
                    use std::mem::{align_of, size_of};
                    assert!(
                        size_of::<Resource<RpcDescriptorWrapper>>()
                            == size_of::<
                                Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                            >()
                    );
                    assert!(
                        align_of::<Resource<RpcDescriptorWrapper>>()
                            == align_of::<
                                Resource<wasmtime_wasi::bindings::filesystem::types::Descriptor>,
                            >()
                    );
                };
                let host_descriptor = unsafe {
                    std::mem::transmute::<
                        Resource<RpcDescriptorWrapper>,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_readlink_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_remove_directory_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_rename_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_symlink_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_unlink_file_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_is_same_object(&mut *lock_rpc_store(&core.rpc_store)?, vfs_desc, vfs_other)?;

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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_metadata_hash(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
            .rpc_instance
            .wasi_filesystem_types()
            .descriptor()
            .call_metadata_hash_at(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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

impl wasmtime_wasi::bindings::sync::filesystem::types::HostDirectoryEntryStream
    for VfsRpcHostState
{
    fn read_directory_entry(
        &mut self,
        self_: Resource<wasmtime_wasi::bindings::filesystem::types::DirectoryEntryStream>,
    ) -> Result<
        Option<wasmtime_wasi::bindings::sync::filesystem::types::DirectoryEntry>,
        TrappableError<wasmtime_wasi::bindings::filesystem::types::ErrorCode>,
    > {
        // Get VFS stream from ResourceTable using wrapper type
        let rep = self_.rep();
        let wrapper_resource: Resource<RpcDirectoryEntryStreamWrapper> = Resource::new_borrow(rep);
        let wrapper = self.table.get(&wrapper_resource).map_err(|e| {
            TrappableError::trap(anyhow::anyhow!(
                "Failed to get directory entry stream from table: {}",
                e
            ))
        })?;
        let vfs_stream = wrapper.0;

        // Lock shared VFS core and call RPC adapter's read_directory_entry
        let core = self.lock_vfs_core().map_err(TrappableError::trap)?;

        let result = core
            .rpc_instance
            .wasi_filesystem_types()
            .directory_entry_stream()
            .call_read_directory_entry(
                &mut *lock_rpc_store(&core.rpc_store).map_err(TrappableError::trap)?,
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
        let wrapper_resource: Resource<RpcDirectoryEntryStreamWrapper> =
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
