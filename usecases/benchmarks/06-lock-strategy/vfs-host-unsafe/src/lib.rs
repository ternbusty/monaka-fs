// VFS Host Implementation using fs-unsafe (no locking)

use anyhow::Result;
use std::sync::Arc;

pub use fs_unsafe::Fs;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

pub mod filesystem_preopens;
pub mod filesystem_types;

/// Wrapper for file descriptor stored in ResourceTable.
#[derive(Clone)]
pub struct FsDescriptorWrapper {
    pub fd: u32,
    pub path: Option<String>,
}

/// Wrapper for directory entry stream stored in ResourceTable.
#[derive(Clone)]
pub struct FsDirectoryEntryStreamWrapper {
    pub entries: Vec<(String, bool)>,
    pub position: usize,
}

/// Host state using fs-unsafe (no-lock filesystem).
/// WARNING: This will cause data races under concurrent access.
pub struct VfsHostState {
    pub wasi_ctx: WasiCtx,
    pub table: ResourceTable,
    pub shared_vfs: Arc<Fs>,
}

impl VfsHostState {
    pub fn new() -> Result<Self> {
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

/// Helper function to convert fs-unsafe error to WASI error code
pub fn convert_fs_error(
    error: fs_unsafe::FsError,
) -> wasmtime_wasi::bindings::sync::filesystem::types::ErrorCode {
    use fs_unsafe::FsError;
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
        FsError::IoError => ErrorCode::Io,
    }
}

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

/// Add WASI interfaces to linker with no-lock VFS filesystem implementation.
pub fn add_to_linker_with_vfs(
    linker: &mut wasmtime::component::Linker<VfsHostState>,
) -> Result<()> {
    use wasmtime_wasi::WasiImpl;

    let wasi_closure = type_annotate_wasi(|t| WasiImpl(t));
    let fs_closure = type_annotate_identity(|t| t);

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

    wasmtime_wasi::bindings::sync::filesystem::types::add_to_linker_get_host(linker, fs_closure)?;
    wasmtime_wasi::bindings::sync::filesystem::preopens::add_to_linker_get_host(
        linker, fs_closure,
    )?;

    let exit_options = wasmtime_wasi::bindings::cli::exit::LinkOptions::default();
    wasmtime_wasi::bindings::cli::exit::add_to_linker_get_host(
        linker,
        &exit_options,
        wasi_closure,
    )?;

    let network_options = wasmtime_wasi::bindings::sockets::network::LinkOptions::default();
    wasmtime_wasi::bindings::sockets::network::add_to_linker_get_host(
        linker,
        &network_options,
        wasi_closure,
    )?;

    Ok(())
}
