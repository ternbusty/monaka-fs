// VFS Host Trait Implementation
//
// This library implements wasmtime Host traits using fs-core directly.
// This enables true dynamic linking where multiple applications can share a single
// VFS instance at runtime, unlike wasi-virt which creates isolated VFS per app.

use anyhow::Result;
use fs_core::Fs;
use std::sync::Arc;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

pub mod filesystem_preopens;
pub mod filesystem_types;

/// Wrapper for fs-core file descriptor stored in ResourceTable.
/// Contains the fd and optionally the path for directory descriptors.
#[derive(Clone)]
pub struct FsDescriptorWrapper {
    /// fs-core file descriptor
    pub fd: u32,
    /// Path for directory descriptors (used for relative path resolution)
    pub path: Option<String>,
}

/// Wrapper for directory entry stream stored in ResourceTable.
/// Caches the directory listing and tracks iteration position.
#[derive(Clone)]
pub struct FsDirectoryEntryStreamWrapper {
    /// Cached directory entries: (name, is_dir)
    pub entries: Vec<(String, bool)>,
    /// Current position in the entries list
    pub position: usize,
}

/// Host state that uses fs-core directly for filesystem operations.
/// Multiple instances can share the same VFS via `Arc<Fs>`.
///
/// Since fs-core uses DashMap internally with fine-grained locking,
/// no external lock is required. All Fs methods take `&self`.
pub struct VfsHostState {
    /// WASI context for host operations (stdio, env, etc.)
    pub wasi_ctx: WasiCtx,

    /// Resource table for managing WASM resources
    pub table: ResourceTable,

    /// Shared VFS: multiple VfsHostState instances can reference the same VFS
    /// No external lock needed - fs-core uses DashMap for internal thread safety
    pub shared_vfs: Arc<Fs>,
}

impl VfsHostState {
    /// Create a new VfsHostState with a fresh fs-core filesystem
    pub fn new() -> Result<Self> {
        // Create host WASI context
        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_stderr()
            .build();

        Ok(Self {
            wasi_ctx,
            table: ResourceTable::new(),
            shared_vfs: Arc::new(Fs::new()),
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

    /// Create a new VfsHostState that shares the same VFS core with custom environment variables
    /// This enables passing configuration to WASM handlers
    pub fn clone_shared_with_env(&self, env_vars: &[(&str, &str)]) -> Self {
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio().inherit_stderr();

        for (key, value) in env_vars {
            builder.env(key, value);
        }

        Self {
            wasi_ctx: builder.build(),
            table: ResourceTable::new(),
            shared_vfs: Arc::clone(&self.shared_vfs),
        }
    }

    /// Create a new VfsHostState from an existing shared VFS with custom environment variables
    /// This is useful when sharing VFS across threads (e.g., in HTTP server handlers)
    pub fn from_shared_vfs_with_env(shared_vfs: Arc<Fs>, env_vars: &[(&str, &str)]) -> Self {
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio().inherit_stderr();

        for (key, value) in env_vars {
            builder.env(key, value);
        }

        Self {
            wasi_ctx: builder.build(),
            table: ResourceTable::new(),
            shared_vfs,
        }
    }

    /// Get the shared VFS for external use (e.g., sharing across threads)
    pub fn get_shared_vfs(&self) -> Arc<Fs> {
        Arc::clone(&self.shared_vfs)
    }
}

impl Default for VfsHostState {
    fn default() -> Self {
        Self::new().expect("Failed to create VfsHostState")
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

/// Helper function to convert fs-core error to WASI error code
pub fn convert_fs_error(
    error: fs_core::FsError,
) -> wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode {
    use fs_core::FsError;
    use wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode;

    match error {
        FsError::NotFound => ErrorCode::NoEntry,
        FsError::NotADirectory => ErrorCode::NotDirectory,
        FsError::IsADirectory => ErrorCode::IsDirectory,
        FsError::AlreadyExists => ErrorCode::Exist,
        FsError::NotEmpty => ErrorCode::NotEmpty,
        FsError::BadFileDescriptor => ErrorCode::BadDescriptor,
        FsError::PermissionDenied => ErrorCode::Access,
        FsError::InvalidArgument => ErrorCode::Invalid,
    }
}

// Public API exports
// Users can import VfsHostState and related types from vfs_host crate root

// Helper to annotate closure type for lifetime inference (same pattern as wasmtime-wasi)
fn type_annotate_wasi<F>(val: F) -> F
where
    F: Fn(&mut VfsHostState) -> wasmtime_wasi::WasiImpl<&mut VfsHostState>,
{
    val
}

fn type_annotate_identity<F>(val: F) -> F
where
    F: Fn(&mut VfsHostState) -> &mut VfsHostState,
{
    val
}

/// Add WASI interfaces to linker with custom VFS filesystem implementation.
///
/// This function registers all standard WASI interfaces but replaces the
/// filesystem implementation with VfsHostState's custom Host trait implementation.
/// This allows std::fs to work transparently through the VFS.
pub fn add_to_linker_with_vfs(
    linker: &mut wasmtime::component::Linker<VfsHostState>,
) -> Result<()> {
    use wasmtime_wasi::WasiImpl;

    // Closure for standard WASI implementations (via WasiImpl)
    let wasi_closure = type_annotate_wasi(|t| WasiImpl(t));

    // Closure for custom filesystem (returns VfsHostState directly)
    let fs_closure = type_annotate_identity(|t| t);

    // Register standard WASI interfaces (non-filesystem)
    wasmtime_wasi::bindings::clocks::wall_clock::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::clocks::monotonic_clock::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::io::error::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::sync::io::poll::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::sync::io::streams::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::random::random::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::random::insecure::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::random::insecure_seed::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::environment::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::stdin::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::stdout::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::stderr::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::terminal_input::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::terminal_output::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::terminal_stdin::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::terminal_stdout::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::cli::terminal_stderr::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::sync::sockets::tcp::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::sockets::tcp_create_socket::add_to_linker_get_host(
        linker,
        wasi_closure,
    )?;
    wasmtime_wasi::bindings::sync::sockets::udp::add_to_linker_get_host(linker, wasi_closure)?;
    wasmtime_wasi::bindings::sockets::udp_create_socket::add_to_linker_get_host(
        linker,
        wasi_closure,
    )?;
    wasmtime_wasi::bindings::sockets::instance_network::add_to_linker_get_host(
        linker,
        wasi_closure,
    )?;
    wasmtime_wasi::bindings::sockets::ip_name_lookup::add_to_linker_get_host(linker, wasi_closure)?;

    // Register custom VFS filesystem implementation
    // These use VfsHostState's Host trait implementations directly
    wasmtime_wasi::bindings::sync::filesystem::types::add_to_linker_get_host(linker, fs_closure)?;
    wasmtime_wasi::bindings::sync::filesystem::preopens::add_to_linker_get_host(
        linker, fs_closure,
    )?;

    // cli::exit requires LinkOptions. Use default
    let exit_options = wasmtime_wasi::bindings::cli::exit::LinkOptions::default();
    wasmtime_wasi::bindings::cli::exit::add_to_linker_get_host(
        linker,
        &exit_options,
        wasi_closure,
    )?;

    // sockets::network requires LinkOptions. Use default
    let network_options = wasmtime_wasi::bindings::sockets::network::LinkOptions::default();
    wasmtime_wasi::bindings::sockets::network::add_to_linker_get_host(
        linker,
        &network_options,
        wasi_closure,
    )?;

    Ok(())
}
