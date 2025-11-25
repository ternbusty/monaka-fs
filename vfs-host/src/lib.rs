// VFS Host Trait Implementation
//
// This library implements wasmtime Host traits that wrap a VfsAdapter component instance.
// This enables true dynamic linking where multiple applications can share a single
// VFS instance at runtime, unlike wasi-virt which creates isolated VFS per app.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::{bindgen, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

// Generate bindings for the VFS adapter world
bindgen!({
    path: "../wit",
    world: "vfs-adapter",
    async: false,
});

pub mod filesystem_preopens;
pub mod filesystem_types;

/// Core VFS state that is shared across multiple applications
/// This is wrapped in Arc<Mutex<>> to enable concurrent access
pub struct SharedVfsCore {
    /// The VFS adapter component instance
    pub vfs_instance: VfsAdapter,

    /// Dedicated store for VFS adapter operations (separately locked to avoid borrow issues)
    pub vfs_store: Arc<Mutex<Store<VfsStoreData>>>,

    /// Maps host Descriptor resources (rep) to VFS Descriptor resources
    pub descriptor_map: HashMap<u32, crate::exports::wasi::filesystem::types::Descriptor>,

    /// Maps host DirectoryEntryStream resources (rep) to VFS DirectoryEntryStream resources
    pub dir_stream_map: HashMap<u32, crate::exports::wasi::filesystem::types::DirectoryEntryStream>,

    /// Maps host InputStream resources (rep) to VFS InputStream resources
    pub input_stream_map: HashMap<u32, crate::exports::wasi::io::streams::InputStream>,

    /// Maps host OutputStream resources (rep) to VFS OutputStream resources
    pub output_stream_map: HashMap<u32, crate::exports::wasi::io::streams::OutputStream>,
}

/// Host state that wraps a VfsAdapter component instance
/// and implements WASI Host traits to forward calls to the VFS component.
/// Multiple instances can share the same VFS core via Arc<Mutex<>>.
pub struct VfsHostState {
    /// WASI context for host operations (stdio, env, etc.)
    pub wasi_ctx: WasiCtx,

    /// Resource table for managing WASM resources
    pub table: ResourceTable,

    /// Shared VFS core - multiple VfsHostState instances can reference the same VFS
    pub shared_vfs: Arc<Mutex<SharedVfsCore>>,
}

/// Data stored in the VFS-specific store
pub struct VfsStoreData {
    pub wasi_ctx: WasiCtx,
    pub table: ResourceTable,
}

impl WasiView for VfsStoreData {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl VfsHostState {
    /// Create a new VfsHostState by instantiating a VFS adapter component
    pub fn new(engine: &Engine, vfs_adapter_path: &str) -> Result<Self> {
        // Create WASI context for the VFS store
        let vfs_wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_stderr()
            .build();

        let vfs_store_data = VfsStoreData {
            wasi_ctx: vfs_wasi_ctx,
            table: ResourceTable::new(),
        };

        let mut vfs_store = Store::new(engine, vfs_store_data);

        // Create linker for VFS adapter
        let mut vfs_linker = wasmtime::component::Linker::new(engine);
        wasmtime_wasi::add_to_linker_sync(&mut vfs_linker)?;

        // Load and instantiate VFS adapter
        let vfs_component = wasmtime::component::Component::from_file(engine, vfs_adapter_path)?;
        let vfs_instance = VfsAdapter::instantiate(&mut vfs_store, &vfs_component, &vfs_linker)?;

        // Create shared VFS core
        let shared_vfs_core = SharedVfsCore {
            vfs_instance,
            vfs_store: Arc::new(Mutex::new(vfs_store)),
            descriptor_map: HashMap::new(),
            dir_stream_map: HashMap::new(),
            input_stream_map: HashMap::new(),
            output_stream_map: HashMap::new(),
        };

        // Create host WASI context
        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_stderr()
            .build();

        Ok(Self {
            wasi_ctx,
            table: ResourceTable::new(),
            shared_vfs: Arc::new(Mutex::new(shared_vfs_core)),
        })
    }

    /// Create a new VfsHostState that shares the same VFS core
    /// This enables multiple applications to access the same VFS concurrently
    pub fn clone_shared(&self) -> Self {
        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_stderr()
            .build();

        Self {
            wasi_ctx,
            table: ResourceTable::new(),
            shared_vfs: Arc::clone(&self.shared_vfs),
        }
    }
}

impl WasiView for VfsHostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

/// Helper function to convert VFS error codes to WASI error codes
pub fn convert_vfs_error(
    error: crate::exports::wasi::filesystem::types::ErrorCode,
) -> wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode {
    use crate::exports::wasi::filesystem::types::ErrorCode as VfsError;
    use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode as WasiError;

    match error {
        VfsError::Access => WasiError::Access,
        VfsError::WouldBlock => WasiError::WouldBlock,
        VfsError::Already => WasiError::Already,
        VfsError::BadDescriptor => WasiError::BadDescriptor,
        VfsError::Busy => WasiError::Busy,
        VfsError::Deadlock => WasiError::Deadlock,
        VfsError::Quota => WasiError::Quota,
        VfsError::Exist => WasiError::Exist,
        VfsError::FileTooLarge => WasiError::FileTooLarge,
        VfsError::IllegalByteSequence => WasiError::IllegalByteSequence,
        VfsError::InProgress => WasiError::InProgress,
        VfsError::Interrupted => WasiError::Interrupted,
        VfsError::Invalid => WasiError::Invalid,
        VfsError::Io => WasiError::Io,
        VfsError::IsDirectory => WasiError::IsDirectory,
        VfsError::Loop => WasiError::Loop,
        VfsError::TooManyLinks => WasiError::TooManyLinks,
        VfsError::MessageSize => WasiError::MessageSize,
        VfsError::NameTooLong => WasiError::NameTooLong,
        VfsError::NoDevice => WasiError::NoDevice,
        VfsError::NoEntry => WasiError::NoEntry,
        VfsError::NoLock => WasiError::NoLock,
        VfsError::InsufficientMemory => WasiError::InsufficientMemory,
        VfsError::InsufficientSpace => WasiError::InsufficientSpace,
        VfsError::NotDirectory => WasiError::NotDirectory,
        VfsError::NotEmpty => WasiError::NotEmpty,
        VfsError::NotRecoverable => WasiError::NotRecoverable,
        VfsError::Unsupported => WasiError::Unsupported,
        VfsError::NoTty => WasiError::NoTty,
        VfsError::NoSuchDevice => WasiError::NoSuchDevice,
        VfsError::Overflow => WasiError::Overflow,
        VfsError::NotPermitted => WasiError::NotPermitted,
        VfsError::Pipe => WasiError::Pipe,
        VfsError::ReadOnly => WasiError::ReadOnly,
        VfsError::InvalidSeek => WasiError::InvalidSeek,
        VfsError::TextFileBusy => WasiError::TextFileBusy,
        VfsError::CrossDevice => WasiError::CrossDevice,
    }
}

// Public API exports
// Users can import VfsHostState and related types from vfs_host crate root
