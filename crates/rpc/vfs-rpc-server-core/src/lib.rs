//! Pure RPC request-handling logic for the VFS RPC server.
//!
//! This crate is the "logic layer" extracted from `vfs-rpc-server`, the
//! cdylib component that runs on `wasm32-wasip2`. The cdylib depends on
//! this crate, hands incoming protobuf-decoded `Request`s to
//! [`ServerContext::handle_request`], and ships the resulting `Response`
//! back over its TCP listener.
//!
//! Splitting these two concerns has a single concrete benefit: this crate
//! is plain Rust (no `wit-bindgen`, no `cdylib`), so it builds and tests
//! under `cargo test --workspace` against the native host like any other
//! library. The cdylib stays minimal — TCP I/O loop, WASI imports and a
//! UUID generator using `wasi:random/random` — and is exercised through
//! the e2e suite.
//!
//! # S3 file locks
//!
//! With the `s3-sync` feature and `VFS_S3_FILE_LOCK=enabled`, opening a
//! file for write acquires a per-file S3 lease before the local open, and
//! closing the last write descriptor for that path pushes the content with
//! `If-Match` and releases the lease. The server never waits for a lease:
//! `lock_timeout` is forced to zero so a single blocked handler cannot
//! stall the single-threaded event loop, and the rpc-adapter retries an
//! `ErrorCode::Busy` open with backoff instead.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use fs_core::{Fs, FsError, MonotonicCounter};
use vfs_rpc_protocol::{
    negotiate_version, DirEntry, ErrorCode, FileMetadata, Request, Response, PROTOCOL_VERSION,
};

#[cfg(feature = "s3-sync")]
use std::sync::Arc;
#[cfg(feature = "s3-sync")]
use vfs_sync_adapter::{
    init_from_s3, new_s3_storage, MetadataCache, SyncConfig, SyncError, SyncManager,
};

/// Server context holding shared state. Constructed once via
/// [`init_server`] and shared across all client handlers.
pub struct ServerContext {
    pub fs: Rc<RefCell<Fs<MonotonicCounter>>>,
    #[cfg(feature = "s3-sync")]
    pub sync_manager: Option<SyncManager<MonotonicCounter>>,
    /// Map from file descriptor to path for sync tracking.
    pub fd_path_map: RefCell<HashMap<u32, String>>,
    /// Map from file descriptor to the fs-core open flags it was opened
    /// with, so close can tell write descriptors from read descriptors.
    pub fd_flags_map: RefCell<HashMap<u32, u32>>,
    /// Descriptors opened by each session, so a disconnect can close what
    /// the client left open.
    pub session_fds: RefCell<HashMap<String, HashSet<u32>>>,
    /// Track which fds have been refreshed from S3 (to avoid repeated
    /// refreshes within the lifetime of an fd).
    pub s3_refreshed_fds: RefCell<HashSet<u32>>,
    /// Whether to refresh files from S3 on read (`VFS_READ_MODE=s3`).
    #[cfg(feature = "s3-sync")]
    pub read_from_s3: bool,
    /// Whether to check S3 metadata on open (`VFS_METADATA_MODE=s3`).
    #[cfg(feature = "s3-sync")]
    pub metadata_sync: bool,
}

/// Whether fs-core open flags request write access.
pub fn is_write_access(flags: u32) -> bool {
    flags & 0x3 != fs_core::O_RDONLY
}

/// Absolute path of `path` opened relative to the directory at `dir_path`.
fn join_dir(dir_path: &str, path: &str) -> String {
    if dir_path == "/" {
        format!("/{}", path.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            dir_path.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

impl ServerContext {
    fn register_fd(&self, fd: u32, path: Option<String>, flags: u32, session: Option<&str>) {
        if let Some(path) = path {
            self.fd_path_map.borrow_mut().insert(fd, path);
        }
        self.fd_flags_map.borrow_mut().insert(fd, flags);
        if let Some(sid) = session {
            self.session_fds
                .borrow_mut()
                .entry(sid.to_string())
                .or_default()
                .insert(fd);
        }
    }

    /// Forget `fd` in every map. Returns its path and flags when known.
    fn unregister_fd(&self, fd: u32) -> (Option<String>, u32) {
        let path = self.fd_path_map.borrow_mut().remove(&fd);
        let flags = self.fd_flags_map.borrow_mut().remove(&fd).unwrap_or(0);
        self.s3_refreshed_fds.borrow_mut().remove(&fd);
        for fds in self.session_fds.borrow_mut().values_mut() {
            fds.remove(&fd);
        }
        (path, flags)
    }

    /// Acquire the S3 lease for a write open. Returns an error response
    /// when the lease is busy or the refresh failed.
    #[cfg(feature = "s3-sync")]
    async fn before_open(&self, path: &str, flags: u32) -> Option<Response> {
        let sync = self.sync_manager.as_ref()?;
        if sync.file_lock_enabled() && is_write_access(flags) {
            if let Err(e) = sync.on_open_write(path).await {
                return Some(map_sync_error(e));
            }
        } else if self.metadata_sync {
            match sync.check_and_refresh_from_s3(path).await {
                Ok(true) => log::debug!("[sync] Refreshed on open: {}", path),
                Ok(false) => {}
                Err(e) => log::error!("[sync] Metadata check failed for {}: {}", path, e),
            }
        }
        None
    }

    #[cfg(not(feature = "s3-sync"))]
    async fn before_open(&self, _path: &str, _flags: u32) -> Option<Response> {
        None
    }

    /// Close `fd` locally and, for a leased write descriptor, push its
    /// content and release the lease.
    pub async fn close_fd(&self, fd: u32) -> Response {
        let (path, flags) = self.unregister_fd(fd);
        let local = self.fs.borrow_mut().close(fd);

        #[cfg(feature = "s3-sync")]
        if let (Some(path), Some(sync)) = (path.as_ref(), self.sync_manager.as_ref()) {
            if is_write_access(flags) {
                if let Err(e) = sync.on_close(path).await {
                    log::warn!("[sync] Close of {} reported: {}", path, e);
                    return map_sync_error(e);
                }
            }
        }
        #[cfg(not(feature = "s3-sync"))]
        let _ = (path, flags);

        match local {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        }
    }

    /// Close every descriptor a session left open (client disconnect).
    pub async fn close_session(&self, session_id: &str) {
        let fds: Vec<u32> = self
            .session_fds
            .borrow_mut()
            .remove(session_id)
            .map(|set| set.into_iter().collect())
            .unwrap_or_default();
        for fd in fds {
            if let Response::Error { message, .. } = self.close_fd(fd).await {
                log::warn!(
                    "[session {}] Closing fd {} on disconnect failed: {}",
                    session_id,
                    fd,
                    message
                );
            }
        }
    }

    /// Handle a single RPC request and return the response.
    ///
    /// `session_id` is the client's existing session id (or `None` before
    /// `Connect`). `new_session_id` is pre-generated by the caller and is
    /// used **only** to populate the response for `Request::Connect`; for
    /// all other request kinds it is ignored. The caller pre-generates so
    /// that the source of randomness (`wasi:random` on WASI, an OS RNG on
    /// host tests) is kept outside this pure library.
    pub async fn handle_request(
        &self,
        request: Request,
        session_id: Option<String>,
        new_session_id: String,
    ) -> Response {
        if let Some(ref sid) = session_id {
            log::debug!("[session {}] Processing request", sid);
        }

        match request {
            Request::Connect { version } => match negotiate_version(version) {
                Some(negotiated) => {
                    log::info!(
                        "[session {}] New client connected (protocol v{})",
                        new_session_id,
                        negotiated
                    );
                    Response::Connected {
                        session_id: new_session_id,
                        version: negotiated,
                    }
                }
                None => Response::Error {
                    code: ErrorCode::ProtocolError,
                    message: format!(
                        "Protocol version mismatch: client={}, server={}",
                        version, PROTOCOL_VERSION
                    ),
                },
            },

            Request::OpenPath { path, flags } => {
                if let Some(err) = self.before_open(&path, flags).await {
                    return err;
                }

                match self.fs.borrow_mut().open_path_with_flags(&path, flags) {
                    Ok(fd) => {
                        self.register_fd(fd, Some(path), flags, session_id.as_deref());
                        Response::Fd { fd }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::OpenAt {
                dir_fd,
                path,
                flags,
            } => {
                let abs_path = self
                    .fd_path_map
                    .borrow()
                    .get(&dir_fd)
                    .map(|dir| join_dir(dir, &path));

                if let Some(abs) = abs_path.as_ref() {
                    if let Some(err) = self.before_open(abs, flags).await {
                        return err;
                    }
                }

                match self.fs.borrow_mut().open_at(dir_fd, &path, flags) {
                    Ok(fd) => {
                        self.register_fd(fd, abs_path, flags, session_id.as_deref());
                        Response::Fd { fd }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Read { fd, length } => {
                // Refresh from S3 before read (once per fd, only when read-through is enabled).
                #[cfg(feature = "s3-sync")]
                if self.read_from_s3 && !self.s3_refreshed_fds.borrow().contains(&fd) {
                    let path = self.fd_path_map.borrow().get(&fd).cloned();
                    if let Some(path) = path {
                        if let Some(ref sync) = self.sync_manager {
                            if let Err(e) = sync.refresh_file_from_s3(&path).await {
                                log::error!("[sync] Refresh failed for {}: {}", path, e);
                            }
                        }
                    }
                    self.s3_refreshed_fds.borrow_mut().insert(fd);
                }

                let mut buf = vec![0u8; length];
                match self.fs.borrow_mut().read(fd, &mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Response::Data { bytes: buf }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Write { fd, data } => {
                let result = self.fs.borrow_mut().write(fd, &data);
                match result {
                    Ok(n) => {
                        self.after_write(fd).await;
                        Response::Written { count: n }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::AppendWrite { fd, data } => {
                let result = self.fs.borrow_mut().append_write(fd, &data);
                match result {
                    Ok(n) => {
                        self.after_write(fd).await;
                        Response::Written { count: n }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Ftruncate { fd, size } => {
                let result = self.fs.borrow_mut().ftruncate(fd, size);
                match result {
                    Ok(()) => {
                        self.after_write(fd).await;
                        Response::Ok
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Close { fd } => self.close_fd(fd).await,

            Request::Fsync { fd } => {
                if !self.fd_flags_map.borrow().contains_key(&fd) {
                    return map_fs_error(FsError::BadFileDescriptor);
                }
                #[cfg(feature = "s3-sync")]
                {
                    let path = self.fd_path_map.borrow().get(&fd).cloned();
                    let flags = self.fd_flags_map.borrow().get(&fd).copied().unwrap_or(0);
                    if let (Some(path), Some(sync)) = (path, self.sync_manager.as_ref()) {
                        if is_write_access(flags) {
                            if let Err(e) = sync.on_fsync(&path).await {
                                return map_sync_error(e);
                            }
                        }
                    }
                }
                Response::Ok
            }

            Request::Seek { fd, offset, whence } => {
                match self.fs.borrow_mut().seek(fd, offset, whence) {
                    Ok(pos) => Response::Position { pos },
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Fstat { fd } => match self.fs.borrow().fstat(fd) {
                Ok(meta) => Response::Metadata {
                    metadata: FileMetadata {
                        size: meta.size,
                        created: meta.created,
                        modified: meta.modified,
                        is_dir: meta.is_dir,
                    },
                },
                Err(e) => map_fs_error(e),
            },

            Request::Stat { path } => match self.fs.borrow().stat(&path) {
                Ok(meta) => Response::Metadata {
                    metadata: FileMetadata {
                        size: meta.size,
                        created: meta.created,
                        modified: meta.modified,
                        is_dir: meta.is_dir,
                    },
                },
                Err(e) => map_fs_error(e),
            },

            Request::Mkdir { path } => {
                let result = self.fs.borrow_mut().mkdir(&path);
                match result {
                    Ok(()) => {
                        self.after_mkdir(&path).await;
                        Response::Ok
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::MkdirP { path } => {
                let result = self.fs.borrow_mut().mkdir_p(&path);
                match result {
                    Ok(()) => {
                        self.after_mkdir(&path).await;
                        Response::Ok
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Unlink { path } => {
                let result = self.fs.borrow_mut().unlink(&path);
                match result {
                    Ok(()) => {
                        #[cfg(feature = "s3-sync")]
                        if let Some(ref sync) = self.sync_manager {
                            sync.enqueue_delete(path.clone());
                        }
                        Response::Ok
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Readdir { path } => match self.fs.borrow().readdir(&path) {
                Ok(names) => {
                    let fs = self.fs.borrow();
                    let mut entries = Vec::new();
                    for name in names {
                        let full_path = if path == "/" {
                            format!("/{}", name)
                        } else {
                            format!("{}/{}", path, name)
                        };
                        let is_dir = fs.stat(&full_path).map(|meta| meta.is_dir).unwrap_or(false);
                        entries.push(DirEntry { name, is_dir });
                    }
                    Response::DirEntries { entries }
                }
                Err(e) => map_fs_error(e),
            },

            Request::ReaddirFd { fd } => match self.fs.borrow().readdir_fd(fd) {
                Ok(entries) => {
                    let dir_entries = entries
                        .into_iter()
                        .map(|(name, is_dir)| DirEntry { name, is_dir })
                        .collect();
                    Response::DirEntries {
                        entries: dir_entries,
                    }
                }
                Err(e) => map_fs_error(e),
            },

            Request::Rmdir { path } => {
                let result = self.fs.borrow_mut().rmdir(&path);
                match result {
                    Ok(()) => {
                        self.after_rmdir(&path).await;
                        Response::Ok
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Rename { old_path, new_path } => {
                match self.fs.borrow_mut().rename(&old_path, &new_path) {
                    Ok(()) => Response::Ok,
                    Err(e) => map_fs_error(e),
                }
            }
        }
    }

    /// Outbound sync after a successful local write. Under a lease the
    /// manager only marks the path dirty (batch) or pushes with `If-Match`
    /// (realtime); either way close is where the lease is released.
    #[cfg(feature = "s3-sync")]
    async fn after_write(&self, fd: u32) {
        let Some(sync) = self.sync_manager.as_ref() else {
            return;
        };
        let path = self.fd_path_map.borrow().get(&fd).cloned();
        let Some(path) = path else {
            return;
        };
        if sync.is_realtime() {
            if let Err(e) = sync.sync_file_now(&path).await {
                log::error!("[sync] RealTime sync failed: {}", e);
            }
        } else {
            sync.enqueue_upload(path);
        }
    }

    #[cfg(not(feature = "s3-sync"))]
    async fn after_write(&self, _fd: u32) {}

    #[cfg(feature = "s3-sync")]
    async fn after_mkdir(&self, path: &str) {
        if let Some(ref sync) = self.sync_manager {
            if let Err(e) = sync.on_mkdir(path).await {
                log::error!("[sync] mkdir marker for {} failed: {}", path, e);
            }
        }
    }

    #[cfg(not(feature = "s3-sync"))]
    async fn after_mkdir(&self, _path: &str) {}

    #[cfg(feature = "s3-sync")]
    async fn after_rmdir(&self, path: &str) {
        if let Some(ref sync) = self.sync_manager {
            if let Err(e) = sync.on_rmdir(path).await {
                log::error!("[sync] rmdir marker for {} failed: {}", path, e);
            }
        }
    }

    #[cfg(not(feature = "s3-sync"))]
    async fn after_rmdir(&self, _path: &str) {}
}

/// Map a `fs-core` error to the protocol's error response.
pub fn map_fs_error(error: FsError) -> Response {
    let (code, message) = match error {
        FsError::NotFound => (ErrorCode::NotFound, "Not found"),
        FsError::NotADirectory => (ErrorCode::NotADirectory, "Not a directory"),
        FsError::IsADirectory => (ErrorCode::IsADirectory, "Is a directory"),
        FsError::InvalidArgument => (ErrorCode::InvalidArgument, "Invalid argument"),
        FsError::BadFileDescriptor => (ErrorCode::BadFileDescriptor, "Bad file descriptor"),
        FsError::PermissionDenied => (ErrorCode::PermissionDenied, "Permission denied"),
        FsError::AlreadyExists => (ErrorCode::AlreadyExists, "Already exists"),
        FsError::NotEmpty => (ErrorCode::NotEmpty, "Directory not empty"),
    };

    Response::Error {
        code,
        message: message.to_string(),
    }
}

/// Map a sync-layer error to the protocol's error response.
#[cfg(feature = "s3-sync")]
pub fn map_sync_error(error: SyncError) -> Response {
    let code = match &error {
        SyncError::Busy { .. } => ErrorCode::Busy,
        SyncError::Conflict { .. } => ErrorCode::Conflict,
        SyncError::Fs { .. } | SyncError::S3(_) => ErrorCode::Io,
    };
    Response::Error {
        code,
        message: error.to_string(),
    }
}

/// Build a fresh [`ServerContext`], optionally pre-populating it from an
/// S3 bucket when the `s3-sync` feature is enabled and `VFS_S3_BUCKET` is
/// set. Reads configuration entirely from environment variables.
pub async fn init_server() -> ServerContext {
    #[cfg(feature = "s3-sync")]
    let s3_bucket = std::env::var("VFS_S3_BUCKET").ok();
    #[cfg(feature = "s3-sync")]
    let s3_prefix = std::env::var("VFS_S3_PREFIX").unwrap_or_else(|_| "vfs/".to_string());

    #[cfg(feature = "s3-sync")]
    let read_from_s3 = std::env::var("VFS_READ_MODE")
        .map(|v| v.to_lowercase() == "s3")
        .unwrap_or(false);
    #[cfg(feature = "s3-sync")]
    let metadata_sync = std::env::var("VFS_METADATA_MODE")
        .map(|v| v.to_lowercase() == "s3")
        .unwrap_or(false);

    #[cfg(feature = "s3-sync")]
    if read_from_s3 {
        log::info!("Read-through mode enabled (VFS_READ_MODE=s3)");
    }
    #[cfg(feature = "s3-sync")]
    if metadata_sync {
        log::info!("Metadata sync mode enabled (VFS_METADATA_MODE=s3)");
    }

    #[cfg(feature = "s3-sync")]
    let (fs, sync_manager) = if let Some(bucket) = s3_bucket {
        log::info!(
            "S3 persistence enabled: bucket={}, prefix={}",
            bucket,
            s3_prefix
        );

        let s3 = Arc::new(new_s3_storage(bucket, s3_prefix).await);

        let (fs, metadata_cache) = match init_from_s3::<MonotonicCounter>(&s3).await {
            Ok((fs, cache)) => (fs, cache),
            Err(e) => {
                log::error!(
                    "Failed to load from S3: {}, starting with empty filesystem",
                    e
                );
                (Rc::new(RefCell::new(Fs::new())), MetadataCache::new())
            }
        };

        let mut config = SyncConfig::from_env();
        // The event loop is single-threaded: a handler that waited for a
        // lease would block every other session, including the one that
        // has to close the file to release it. Try once and let the
        // rpc-adapter retry on `Busy`.
        config.lock_timeout = std::time::Duration::ZERO;
        log::info!(
            "Sync mode: {:?} (set VFS_SYNC_MODE=realtime for immediate sync)",
            config.mode
        );
        log::info!(
            "S3 file lock: {} (VFS_S3_FILE_LOCK)",
            if config.file_lock {
                "enabled"
            } else {
                "disabled"
            }
        );
        let sync_manager = SyncManager::new(
            s3,
            vfs_sync_adapter::AdapterFs(fs.clone()),
            metadata_cache,
            config,
        );

        (fs, Some(sync_manager))
    } else {
        log::info!("S3 persistence disabled (VFS_S3_BUCKET not set)");
        (Rc::new(RefCell::new(Fs::new())), None)
    };

    #[cfg(not(feature = "s3-sync"))]
    let fs = Rc::new(RefCell::new(Fs::new()));

    ServerContext {
        fs,
        #[cfg(feature = "s3-sync")]
        sync_manager,
        fd_path_map: RefCell::new(HashMap::new()),
        fd_flags_map: RefCell::new(HashMap::new()),
        session_fds: RefCell::new(HashMap::new()),
        s3_refreshed_fds: RefCell::new(HashSet::new()),
        #[cfg(feature = "s3-sync")]
        read_from_s3,
        #[cfg(feature = "s3-sync")]
        metadata_sync,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        // Minimal single-thread tokio runtime for tests; the production
        // cdylib also uses `new_current_thread`.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn empty_ctx() -> ServerContext {
        ServerContext {
            fs: Rc::new(RefCell::new(Fs::new())),
            #[cfg(feature = "s3-sync")]
            sync_manager: None,
            fd_path_map: RefCell::new(HashMap::new()),
            fd_flags_map: RefCell::new(HashMap::new()),
            session_fds: RefCell::new(HashMap::new()),
            s3_refreshed_fds: RefCell::new(HashSet::new()),
            #[cfg(feature = "s3-sync")]
            read_from_s3: false,
            #[cfg(feature = "s3-sync")]
            metadata_sync: false,
        }
    }

    fn open(ctx: &ServerContext, path: &str, flags: u32, session: Option<&str>) -> u32 {
        let resp = block_on(ctx.handle_request(
            Request::OpenPath {
                path: path.into(),
                flags,
            },
            session.map(|s| s.to_string()),
            "irrelevant".into(),
        ));
        match resp {
            Response::Fd { fd } => fd,
            other => panic!("expected Fd, got {:?}", other),
        }
    }

    #[test]
    fn map_fs_error_translates_each_variant() {
        let cases = [
            (FsError::NotFound, ErrorCode::NotFound, "Not found"),
            (
                FsError::NotADirectory,
                ErrorCode::NotADirectory,
                "Not a directory",
            ),
            (
                FsError::IsADirectory,
                ErrorCode::IsADirectory,
                "Is a directory",
            ),
            (
                FsError::InvalidArgument,
                ErrorCode::InvalidArgument,
                "Invalid argument",
            ),
            (
                FsError::BadFileDescriptor,
                ErrorCode::BadFileDescriptor,
                "Bad file descriptor",
            ),
            (
                FsError::PermissionDenied,
                ErrorCode::PermissionDenied,
                "Permission denied",
            ),
            (
                FsError::AlreadyExists,
                ErrorCode::AlreadyExists,
                "Already exists",
            ),
            (
                FsError::NotEmpty,
                ErrorCode::NotEmpty,
                "Directory not empty",
            ),
        ];
        for (err, expected_code, expected_message) in cases {
            match map_fs_error(err) {
                Response::Error { code, message } => {
                    assert_eq!(code, expected_code);
                    assert_eq!(message, expected_message);
                }
                _ => panic!("expected Response::Error for {:?}", err),
            }
        }
    }

    #[cfg(feature = "s3-sync")]
    #[test]
    fn map_sync_error_translates_each_variant() {
        let busy = map_sync_error(SyncError::Busy {
            path: "/a".into(),
            holder: None,
        });
        assert!(matches!(
            busy,
            Response::Error {
                code: ErrorCode::Busy,
                ..
            }
        ));
        let conflict = map_sync_error(SyncError::Conflict {
            path: "/a".into(),
            refreshed: true,
        });
        assert!(matches!(
            conflict,
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ));
        let fs = map_sync_error(SyncError::Fs {
            path: "/a".into(),
            message: "x".into(),
        });
        assert!(matches!(
            fs,
            Response::Error {
                code: ErrorCode::Io,
                ..
            }
        ));
    }

    #[test]
    fn connect_with_matching_version_returns_session_id() {
        let ctx = empty_ctx();
        let req = Request::Connect {
            version: PROTOCOL_VERSION,
        };
        let resp = block_on(ctx.handle_request(req, None, "test-session".into()));
        match resp {
            Response::Connected {
                session_id,
                version,
            } => {
                assert_eq!(session_id, "test-session");
                assert_eq!(version, PROTOCOL_VERSION);
            }
            other => panic!("expected Connected, got {:?}", other),
        }
    }

    #[test]
    fn connect_negotiates_down_to_v1_client() {
        let ctx = empty_ctx();
        let resp = block_on(ctx.handle_request(Request::Connect { version: 1 }, None, "s".into()));
        match resp {
            Response::Connected { version, .. } => assert_eq!(version, 1),
            other => panic!("expected Connected, got {:?}", other),
        }
    }

    #[test]
    fn connect_with_wrong_version_returns_protocol_error() {
        let ctx = empty_ctx();
        for version in [0, PROTOCOL_VERSION + 1] {
            let resp =
                block_on(ctx.handle_request(Request::Connect { version }, None, "ignored".into()));
            match resp {
                Response::Error { code, .. } => {
                    assert_eq!(code, ErrorCode::ProtocolError);
                }
                other => panic!("expected Error, got {:?}", other),
            }
        }
    }

    #[test]
    fn open_then_write_then_read_round_trips_via_handler() {
        let ctx = empty_ctx();
        let s = || "irrelevant".to_string();

        let fd = open(&ctx, "/hello.txt", fs_core::O_CREAT | fs_core::O_RDWR, None);

        let resp = block_on(ctx.handle_request(
            Request::Write {
                fd,
                data: b"hi".to_vec(),
            },
            None,
            s(),
        ));
        assert!(matches!(resp, Response::Written { count: 2 }));

        let _ = block_on(ctx.handle_request(
            Request::Seek {
                fd,
                offset: 0,
                whence: 0,
            },
            None,
            s(),
        ));

        let resp = block_on(ctx.handle_request(Request::Read { fd, length: 16 }, None, s()));
        match resp {
            Response::Data { bytes } => assert_eq!(bytes, b"hi"),
            other => panic!("expected Data, got {:?}", other),
        }
    }

    #[test]
    fn open_records_path_flags_and_session() {
        let ctx = empty_ctx();
        let fd = open(
            &ctx,
            "/w",
            fs_core::O_CREAT | fs_core::O_WRONLY,
            Some("sess-1"),
        );
        assert_eq!(ctx.fd_path_map.borrow().get(&fd).unwrap(), "/w");
        assert!(is_write_access(
            *ctx.fd_flags_map.borrow().get(&fd).unwrap()
        ));
        assert!(ctx.session_fds.borrow()["sess-1"].contains(&fd));
    }

    #[test]
    fn open_at_records_absolute_path() {
        let ctx = empty_ctx();
        let dir = open(&ctx, "/", fs_core::O_RDONLY, None);
        let resp = block_on(ctx.handle_request(
            Request::OpenAt {
                dir_fd: dir,
                path: "child.txt".into(),
                flags: fs_core::O_CREAT | fs_core::O_WRONLY,
            },
            None,
            "x".into(),
        ));
        let fd = match resp {
            Response::Fd { fd } => fd,
            other => panic!("expected Fd, got {:?}", other),
        };
        assert_eq!(ctx.fd_path_map.borrow().get(&fd).unwrap(), "/child.txt");
    }

    #[test]
    fn close_clears_all_fd_maps() {
        let ctx = empty_ctx();
        let fd = open(
            &ctx,
            "/c",
            fs_core::O_CREAT | fs_core::O_WRONLY,
            Some("sess-1"),
        );
        ctx.s3_refreshed_fds.borrow_mut().insert(fd);
        let resp = block_on(ctx.handle_request(Request::Close { fd }, None, "x".into()));
        assert!(matches!(resp, Response::Ok));
        assert!(!ctx.fd_path_map.borrow().contains_key(&fd));
        assert!(!ctx.fd_flags_map.borrow().contains_key(&fd));
        assert!(!ctx.s3_refreshed_fds.borrow().contains(&fd));
        assert!(!ctx.session_fds.borrow()["sess-1"].contains(&fd));
    }

    #[test]
    fn close_session_closes_every_descriptor_it_opened() {
        let ctx = empty_ctx();
        let a = open(&ctx, "/a", fs_core::O_CREAT | fs_core::O_WRONLY, Some("s1"));
        let b = open(&ctx, "/b", fs_core::O_CREAT | fs_core::O_WRONLY, Some("s1"));
        let other = open(&ctx, "/o", fs_core::O_CREAT | fs_core::O_WRONLY, Some("s2"));

        block_on(ctx.close_session("s1"));

        for fd in [a, b] {
            let resp = block_on(ctx.handle_request(Request::Fstat { fd }, None, "x".into()));
            assert!(matches!(
                resp,
                Response::Error {
                    code: ErrorCode::BadFileDescriptor,
                    ..
                }
            ));
        }
        let resp = block_on(ctx.handle_request(Request::Fstat { fd: other }, None, "x".into()));
        assert!(matches!(resp, Response::Metadata { .. }));
        assert!(!ctx.session_fds.borrow().contains_key("s1"));
    }

    #[test]
    fn fsync_on_unknown_fd_is_bad_descriptor_and_on_known_fd_is_ok() {
        let ctx = empty_ctx();
        let resp = block_on(ctx.handle_request(Request::Fsync { fd: 99 }, None, "x".into()));
        assert!(matches!(
            resp,
            Response::Error {
                code: ErrorCode::BadFileDescriptor,
                ..
            }
        ));
        let fd = open(&ctx, "/f", fs_core::O_CREAT | fs_core::O_WRONLY, None);
        let resp = block_on(ctx.handle_request(Request::Fsync { fd }, None, "x".into()));
        assert!(matches!(resp, Response::Ok));
    }

    #[test]
    fn read_on_unknown_fd_returns_bad_file_descriptor() {
        let ctx = empty_ctx();
        let resp =
            block_on(ctx.handle_request(Request::Read { fd: 42, length: 8 }, None, "x".into()));
        match resp {
            Response::Error { code, .. } => {
                assert_eq!(code, ErrorCode::BadFileDescriptor);
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn unlink_then_stat_returns_not_found() {
        let ctx = empty_ctx();
        let s = || "x".to_string();

        let fd = open(&ctx, "/x", fs_core::O_CREAT | fs_core::O_WRONLY, None);
        let _ = block_on(ctx.handle_request(Request::Close { fd }, None, s()));

        let r = block_on(ctx.handle_request(Request::Unlink { path: "/x".into() }, None, s()));
        assert!(matches!(r, Response::Ok));

        let r = block_on(ctx.handle_request(Request::Stat { path: "/x".into() }, None, s()));
        match r {
            Response::Error { code, .. } => assert_eq!(code, ErrorCode::NotFound),
            other => panic!("expected Error(NotFound), got {:?}", other),
        }
    }

    #[test]
    fn join_dir_handles_root_and_nested() {
        assert_eq!(join_dir("/", "a.txt"), "/a.txt");
        assert_eq!(join_dir("/", "/a.txt"), "/a.txt");
        assert_eq!(join_dir("/d", "a.txt"), "/d/a.txt");
        assert_eq!(join_dir("/d/", "a.txt"), "/d/a.txt");
    }
}
