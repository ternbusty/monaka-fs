// VFS RPC Host Implementation
//
// This library implements wasmtime Host traits that wrap an RpcAdapter component instance.
// The RpcAdapter component handles TCP RPC communication with vfs-rpc-server,
// enabling applications to use std::fs transparently over the network.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::{bindgen, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

// Generate bindings for the RPC adapter world
bindgen!({
    path: "../wit",
    world: "rpc-adapter",
    async: false,
});

pub mod filesystem_preopens;
pub mod filesystem_types;

/// Core RPC adapter state that is shared across multiple applications
/// This is wrapped in Arc<Mutex<>> to enable concurrent access
pub struct SharedRpcAdapterCore {
    /// The RPC adapter component instance
    pub rpc_instance: RpcAdapter,

    /// Dedicated store for RPC adapter operations (separately locked to avoid borrow issues)
    pub rpc_store: Arc<Mutex<Store<RpcStoreData>>>,

    /// Maps host Descriptor resources (rep) to RPC Descriptor resources
    pub descriptor_map: HashMap<u32, crate::exports::wasi::filesystem::types::Descriptor>,

    /// Maps host DirectoryEntryStream resources (rep) to RPC DirectoryEntryStream resources
    pub dir_stream_map: HashMap<u32, crate::exports::wasi::filesystem::types::DirectoryEntryStream>,

    /// Maps host InputStream resources (rep) to RPC InputStream resources
    pub input_stream_map: HashMap<u32, crate::exports::wasi::io::streams::InputStream>,

    /// Maps host OutputStream resources (rep) to RPC OutputStream resources
    pub output_stream_map: HashMap<u32, crate::exports::wasi::io::streams::OutputStream>,
}

/// Host state that wraps an RpcAdapter component instance
/// and implements WASI Host traits to forward calls to the RPC adapter component.
/// Multiple instances can share the same RPC adapter core via Arc<Mutex<>>.
pub struct VfsRpcHostState {
    /// WASI context for host operations (stdio, env, etc.)
    pub wasi_ctx: WasiCtx,

    /// Resource table for managing WASM resources
    pub table: ResourceTable,

    /// Shared RPC adapter core - multiple VfsRpcHostState instances can reference the same adapter
    pub shared_rpc: Arc<Mutex<SharedRpcAdapterCore>>,
}

/// Data stored in the RPC-specific store
pub struct RpcStoreData {
    pub wasi_ctx: WasiCtx,
    pub table: ResourceTable,
}

impl WasiView for RpcStoreData {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl VfsRpcHostState {
    /// Create a new VfsRpcHostState by instantiating an RPC adapter component
    pub fn new(engine: &Engine, rpc_adapter_path: &str) -> Result<Self> {
        // Create WASI context for the RPC store
        let rpc_wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_stderr()
            .inherit_network() // Important: RPC adapter needs network access
            .build();

        let rpc_store_data = RpcStoreData {
            wasi_ctx: rpc_wasi_ctx,
            table: ResourceTable::new(),
        };

        let mut rpc_store = Store::new(engine, rpc_store_data);

        // Create linker for RPC adapter
        let mut rpc_linker = wasmtime::component::Linker::new(engine);
        wasmtime_wasi::add_to_linker_sync(&mut rpc_linker)?;

        // Load and instantiate RPC adapter
        let rpc_component = wasmtime::component::Component::from_file(engine, rpc_adapter_path)?;
        let rpc_instance = RpcAdapter::instantiate(&mut rpc_store, &rpc_component, &rpc_linker)?;

        // Create shared RPC adapter core
        let shared_rpc = Arc::new(Mutex::new(SharedRpcAdapterCore {
            rpc_instance,
            rpc_store: Arc::new(Mutex::new(rpc_store)),
            descriptor_map: HashMap::new(),
            dir_stream_map: HashMap::new(),
            input_stream_map: HashMap::new(),
            output_stream_map: HashMap::new(),
        }));

        Ok(Self {
            wasi_ctx: WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_stderr()
                .build(),
            table: ResourceTable::new(),
            shared_rpc,
        })
    }

    /// Create a new VfsRpcHostState that shares the same RPC adapter instance
    /// This allows multiple applications to share the same RPC connection
    pub fn clone_shared(&self, _engine: &Engine) -> Result<Self> {
        Ok(Self {
            wasi_ctx: WasiCtxBuilder::new()
                .inherit_stdio()
                .inherit_stderr()
                .build(),
            table: ResourceTable::new(),
            shared_rpc: Arc::clone(&self.shared_rpc),
        })
    }
}

impl WasiView for VfsRpcHostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        eprintln!("[VFS-RPC-HOST] WasiView::ctx() called");
        &mut self.wasi_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        eprintln!("[VFS-RPC-HOST] WasiView::table() called");
        &mut self.table
    }
}

// Helper methods for VfsRpcHostState to handle lock poisoning
impl VfsRpcHostState {
    /// Lock shared RPC adapter core with proper error handling for poisoned locks
    pub(crate) fn lock_rpc_core(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, SharedRpcAdapterCore>, anyhow::Error> {
        self.shared_rpc
            .lock()
            .map_err(|e| anyhow::anyhow!("RPC adapter core lock poisoned: {}", e))
    }
}

// Helper function for locking rpc_store
fn lock_rpc_store(
    arc_store: &Arc<Mutex<wasmtime::Store<crate::RpcStoreData>>>,
) -> Result<std::sync::MutexGuard<'_, wasmtime::Store<crate::RpcStoreData>>, anyhow::Error> {
    arc_store
        .lock()
        .map_err(|e| anyhow::anyhow!("RPC store lock poisoned: {}", e))
}

/// Helper function to convert RPC adapter error codes to WASI error codes
pub fn convert_vfs_error(
    error: crate::exports::wasi::filesystem::types::ErrorCode,
) -> wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode {
    use crate::exports::wasi::filesystem::types::ErrorCode as RpcError;
    use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode as WasiError;

    match error {
        RpcError::Access => WasiError::Access,
        RpcError::WouldBlock => WasiError::WouldBlock,
        RpcError::Already => WasiError::Already,
        RpcError::BadDescriptor => WasiError::BadDescriptor,
        RpcError::Busy => WasiError::Busy,
        RpcError::Deadlock => WasiError::Deadlock,
        RpcError::Quota => WasiError::Quota,
        RpcError::Exist => WasiError::Exist,
        RpcError::FileTooLarge => WasiError::FileTooLarge,
        RpcError::IllegalByteSequence => WasiError::IllegalByteSequence,
        RpcError::InProgress => WasiError::InProgress,
        RpcError::Interrupted => WasiError::Interrupted,
        RpcError::Invalid => WasiError::Invalid,
        RpcError::Io => WasiError::Io,
        RpcError::IsDirectory => WasiError::IsDirectory,
        RpcError::Loop => WasiError::Loop,
        RpcError::TooManyLinks => WasiError::TooManyLinks,
        RpcError::MessageSize => WasiError::MessageSize,
        RpcError::NameTooLong => WasiError::NameTooLong,
        RpcError::NoDevice => WasiError::NoDevice,
        RpcError::NoEntry => WasiError::NoEntry,
        RpcError::NoLock => WasiError::NoLock,
        RpcError::InsufficientMemory => WasiError::InsufficientMemory,
        RpcError::InsufficientSpace => WasiError::InsufficientSpace,
        RpcError::NotDirectory => WasiError::NotDirectory,
        RpcError::NotEmpty => WasiError::NotEmpty,
        RpcError::NotRecoverable => WasiError::NotRecoverable,
        RpcError::Unsupported => WasiError::Unsupported,
        RpcError::NoTty => WasiError::NoTty,
        RpcError::NoSuchDevice => WasiError::NoSuchDevice,
        RpcError::Overflow => WasiError::Overflow,
        RpcError::NotPermitted => WasiError::NotPermitted,
        RpcError::Pipe => WasiError::Pipe,
        RpcError::ReadOnly => WasiError::ReadOnly,
        RpcError::InvalidSeek => WasiError::InvalidSeek,
        RpcError::TextFileBusy => WasiError::TextFileBusy,
        RpcError::CrossDevice => WasiError::CrossDevice,
    }
}
