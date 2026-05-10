// VFS Host Trait Implementation
//
// This library implements wasmtime Host traits using fs-core directly.
// This enables true dynamic linking where multiple applications can share a single
// VFS instance at runtime, unlike wasi-virt which creates isolated VFS per app.

use anyhow::Result;
use std::sync::Arc;

// Re-export fs-core types so users don't need to depend on fs-core directly
pub use fs_core::{Fs, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
use wasmtime::component::{HasSelf, ResourceTable};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

pub mod filesystem_preopens;
pub mod filesystem_types;

// S3 sync support (optional)
#[cfg(feature = "s3-sync")]
mod sync_hooks;
#[cfg(feature = "s3-sync")]
pub use sync_hooks::{NoOpSyncHooks, S3SyncHooks, SyncHooks};
#[cfg(feature = "s3-sync")]
pub use vfs_sync_host::{init_from_s3, HostSyncManager, S3Storage, SyncConfig};

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

    /// Optional sync hooks for S3 synchronization
    #[cfg(feature = "s3-sync")]
    pub sync_hooks: Option<Arc<dyn SyncHooks>>,

    /// Optional sync manager for S3 synchronization
    #[cfg(feature = "s3-sync")]
    pub sync_manager: Option<Arc<HostSyncManager>>,

    /// Background sync thread handle
    #[cfg(feature = "s3-sync")]
    sync_thread: Option<std::thread::JoinHandle<()>>,
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
            #[cfg(feature = "s3-sync")]
            sync_hooks: None,
            #[cfg(feature = "s3-sync")]
            sync_manager: None,
            #[cfg(feature = "s3-sync")]
            sync_thread: None,
        })
    }

    /// Create a new VfsHostState with S3 synchronization enabled
    ///
    /// This will:
    /// 1. Initialize S3 storage client
    /// 2. Load existing files from S3
    /// 3. Start a background sync thread
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `prefix` - S3 key prefix (e.g., "vfs/")
    #[cfg(feature = "s3-sync")]
    pub async fn new_with_s3(bucket: String, prefix: String) -> Result<Self> {
        use std::time::Duration;

        log::info!(
            "[vfs-host] Initializing with S3 sync: bucket={}, prefix={}",
            bucket,
            prefix
        );

        // Initialize S3 storage
        let s3 = vfs_sync_host::new_s3_storage(bucket, prefix).await;

        // Load existing files from S3 (returns Arc<Fs> already)
        let (fs, metadata_cache) = init_from_s3(&s3)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize from S3: {}", e))?;

        let s3 = Arc::new(s3);
        let config = SyncConfig::from_env();

        log::info!("[vfs-host] Sync mode: {:?}", config.mode);

        // Check if read-from-S3 mode is enabled
        let read_from_s3 = std::env::var("VFS_READ_MODE")
            .map(|v| v == "s3")
            .unwrap_or(false);
        if read_from_s3 {
            log::info!("[vfs-host] Read mode: S3 (read-through)");
        } else {
            log::info!("[vfs-host] Read mode: memory (default)");
        }

        // Check if metadata sync mode is enabled (like s3fs HEAD requests)
        let metadata_sync = std::env::var("VFS_METADATA_MODE")
            .map(|v| v == "s3")
            .unwrap_or(false);
        if metadata_sync {
            log::info!("[vfs-host] Metadata mode: S3 (HEAD on every open, like s3fs)");
        } else {
            log::info!("[vfs-host] Metadata mode: memory (default)");
        }

        // Create sync manager
        let sync_manager = Arc::new(HostSyncManager::new(
            s3,
            vfs_sync_host::HostFs(fs.clone()),
            metadata_cache,
            config,
        ));

        // Create sync hooks
        let sync_hooks: Arc<dyn SyncHooks> = Arc::new(S3SyncHooks::new_with_options(
            sync_manager.clone(),
            read_from_s3,
            metadata_sync,
        ));

        // Spawn background sync thread
        let sync_thread = {
            let sync = sync_manager.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime for sync thread");

                rt.block_on(async {
                    log::info!("[vfs-host] Background sync thread started");
                    while !sync.is_shutdown() {
                        sync.maybe_sync().await;
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    // Final flush before exit
                    if let Err(e) = sync.force_flush().await {
                        log::error!("[vfs-host] Final flush failed: {}", e);
                    }
                    log::info!("[vfs-host] Background sync thread stopped");
                });
            })
        };

        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_stderr()
            .build();

        Ok(Self {
            wasi_ctx,
            table: ResourceTable::new(),
            shared_vfs: fs,
            sync_hooks: Some(sync_hooks),
            sync_manager: Some(sync_manager),
            sync_thread: Some(sync_thread),
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
            #[cfg(feature = "s3-sync")]
            sync_hooks: self.sync_hooks.clone(),
            #[cfg(feature = "s3-sync")]
            sync_manager: self.sync_manager.clone(),
            #[cfg(feature = "s3-sync")]
            sync_thread: None, // Don't clone the thread handle
        }
    }

    /// Create a new VfsHostState that shares the same VFS core with custom CLI arguments
    /// This enables passing arguments to WASM applications via WASI
    pub fn clone_shared_with_args(&self, args: &[&str]) -> Self {
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio().inherit_stderr();
        builder.args(args);

        Self {
            wasi_ctx: builder.build(),
            table: ResourceTable::new(),
            shared_vfs: Arc::clone(&self.shared_vfs),
            #[cfg(feature = "s3-sync")]
            sync_hooks: self.sync_hooks.clone(),
            #[cfg(feature = "s3-sync")]
            sync_manager: self.sync_manager.clone(),
            #[cfg(feature = "s3-sync")]
            sync_thread: None, // Don't clone the thread handle
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
            #[cfg(feature = "s3-sync")]
            sync_hooks: self.sync_hooks.clone(),
            #[cfg(feature = "s3-sync")]
            sync_manager: self.sync_manager.clone(),
            #[cfg(feature = "s3-sync")]
            sync_thread: None, // Don't clone the thread handle
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
            #[cfg(feature = "s3-sync")]
            sync_hooks: None,
            #[cfg(feature = "s3-sync")]
            sync_manager: None,
            #[cfg(feature = "s3-sync")]
            sync_thread: None,
        }
    }

    /// Get the shared VFS for external use (e.g., sharing across threads)
    pub fn get_shared_vfs(&self) -> Arc<Fs> {
        Arc::clone(&self.shared_vfs)
    }

    /// Gracefully shutdown S3 sync
    #[cfg(feature = "s3-sync")]
    pub fn shutdown_sync(&mut self) {
        if let Some(ref sync) = self.sync_manager {
            log::info!("[vfs-host] Shutting down S3 sync...");
            sync.shutdown();
        }
        if let Some(handle) = self.sync_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(feature = "s3-sync")]
impl Drop for VfsHostState {
    fn drop(&mut self) {
        self.shutdown_sync();
    }
}

impl Default for VfsHostState {
    fn default() -> Self {
        Self::new().expect("Failed to create VfsHostState")
    }
}

impl WasiView for VfsHostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

/// Helper function to convert fs-core error to WASI error code
pub fn convert_fs_error(
    error: fs_core::FsError,
) -> wasmtime_wasi::p2::bindings::sync::filesystem::types::ErrorCode {
    use fs_core::FsError;
    use wasmtime_wasi::p2::bindings::sync::filesystem::types::ErrorCode;

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

/// Add WASI interfaces to linker with custom VFS filesystem implementation.
///
/// This function registers all standard WASI interfaces but replaces the
/// filesystem implementation with VfsHostState's custom Host trait implementation.
/// This allows std::fs to work transparently through the VFS.
pub fn add_to_linker_with_vfs(
    linker: &mut wasmtime::component::Linker<VfsHostState>,
) -> Result<()> {
    use wasmtime_wasi::cli::{WasiCli, WasiCliView};
    use wasmtime_wasi::clocks::{WasiClocks, WasiClocksView};
    use wasmtime_wasi::p2::bindings;
    use wasmtime_wasi::random::{WasiRandom, WasiRandomView};
    use wasmtime_wasi::sockets::{WasiSockets, WasiSocketsView};

    type T = VfsHostState;

    // Standard non-blocking interfaces (no filesystem)
    bindings::clocks::wall_clock::add_to_linker::<T, WasiClocks>(linker, T::clocks)?;
    bindings::clocks::monotonic_clock::add_to_linker::<T, WasiClocks>(linker, T::clocks)?;
    bindings::random::random::add_to_linker::<T, WasiRandom>(linker, T::random)?;
    bindings::random::insecure::add_to_linker::<T, WasiRandom>(linker, T::random)?;
    bindings::random::insecure_seed::add_to_linker::<T, WasiRandom>(linker, T::random)?;
    bindings::cli::stdin::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::stdout::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::stderr::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::environment::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::terminal_input::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::terminal_output::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::terminal_stdin::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::terminal_stdout::add_to_linker::<T, WasiCli>(linker, T::cli)?;
    bindings::cli::terminal_stderr::add_to_linker::<T, WasiCli>(linker, T::cli)?;

    // Sockets (sync variants)
    bindings::sync::sockets::tcp::add_to_linker::<T, WasiSockets>(linker, T::sockets)?;
    bindings::sync::sockets::udp::add_to_linker::<T, WasiSockets>(linker, T::sockets)?;
    bindings::sockets::tcp_create_socket::add_to_linker::<T, WasiSockets>(linker, T::sockets)?;
    bindings::sockets::udp_create_socket::add_to_linker::<T, WasiSockets>(linker, T::sockets)?;
    bindings::sockets::instance_network::add_to_linker::<T, WasiSockets>(linker, T::sockets)?;
    bindings::sockets::ip_name_lookup::add_to_linker::<T, WasiSockets>(linker, T::sockets)?;

    // I/O (sync variants - error/poll/streams operate on ResourceTable)
    wasmtime_wasi_io::bindings::wasi::io::error::add_to_linker::<T, HasSelf<ResourceTable>>(
        linker,
        |t| &mut t.table,
    )?;
    bindings::sync::io::poll::add_to_linker::<T, HasSelf<ResourceTable>>(linker, |t| &mut t.table)?;
    bindings::sync::io::streams::add_to_linker::<T, HasSelf<ResourceTable>>(linker, |t| {
        &mut t.table
    })?;

    // Custom VFS filesystem implementation - implemented on VfsHostState directly
    bindings::sync::filesystem::types::add_to_linker::<T, HasSelf<VfsHostState>>(linker, |t| t)?;
    bindings::sync::filesystem::preopens::add_to_linker::<T, HasSelf<VfsHostState>>(linker, |t| t)?;

    // Interfaces requiring LinkOptions
    let exit_options = bindings::cli::exit::LinkOptions::default();
    bindings::cli::exit::add_to_linker::<T, WasiCli>(linker, &exit_options, T::cli)?;

    let network_options = bindings::sockets::network::LinkOptions::default();
    bindings::sockets::network::add_to_linker::<T, WasiSockets>(
        linker,
        &network_options,
        T::sockets,
    )?;

    Ok(())
}
