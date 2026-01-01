//! VFS RPC Server with S3 Persistence
//!
//! A WebAssembly component that exposes fs-core filesystem over TCP sockets.
//! Multiple clients can connect and share the same in-memory filesystem.
//! Filesystem state is persisted to S3 asynchronously.

#![no_main]
#![allow(warnings)]

mod file_metadata;
mod s3_client;
mod sync_manager;

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use fs_core::{Fs, FsError};
use vfs_rpc_protocol::{
    DirEntry, ErrorCode, Metadata, Request, Response, RpcRequest, PROTOCOL_VERSION,
};

use file_metadata::MetadataCache;
use s3_client::S3Storage;
use sync_manager::{init_from_s3, SyncConfig, SyncManager};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "vfs-rpc-server",
    path: "../../../wit",
    generate_all,
});

use wasi::clocks::monotonic_clock::subscribe_duration;
use wasi::io::poll::poll;
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

/// Result of trying to read a message with timeout
enum ReadResult {
    /// Successfully read a message
    Message(Vec<u8>),
    /// Timeout occurred, no data available
    Timeout,
    /// Client disconnected
    Disconnected,
}

/// Timeout for polling (5 seconds in nanoseconds)
const POLL_TIMEOUT_NS: u64 = 5_000_000_000;

// Session counter (used as part of hash input for uniqueness)
static mut SESSION_COUNTER: u64 = 0;

/// Generate a unique 6-character alphanumeric session ID
fn generate_session_id() -> String {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    unsafe {
        SESSION_COUNTER = SESSION_COUNTER.wrapping_add(1);

        let mut hasher = DefaultHasher::new();
        SESSION_COUNTER.hash(&mut hasher);
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .hash(&mut hasher);

        let hash = hasher.finish();

        // Convert hash to 6-character alphanumeric string
        let mut result = String::with_capacity(6);
        let mut h = hash;
        for _ in 0..6 {
            result.push(CHARSET[(h % 36) as usize] as char);
            h /= 36;
        }
        result
    }
}

/// Server context holding shared state
struct ServerContext {
    fs: Rc<RefCell<Fs>>,
    sync_manager: Option<SyncManager>,
    /// Map from file descriptor to path for sync tracking
    fd_path_map: RefCell<HashMap<u32, String>>,
}

impl ServerContext {
    /// Handle a single RPC request
    fn handle_request(&self, request: Request, session_id: Option<String>) -> Response {
        // Log the session ID for tracking
        if let Some(ref sid) = session_id {
            println!("[session {}] Processing request", sid);
        }

        match request {
            Request::Connect { version } => {
                if version != PROTOCOL_VERSION {
                    Response::Error {
                        code: ErrorCode::ProtocolError,
                        message: format!(
                            "Protocol version mismatch: client={}, server={}",
                            version, PROTOCOL_VERSION
                        ),
                    }
                } else {
                    let new_session_id = generate_session_id();
                    println!("[session {}] New client connected", new_session_id);
                    Response::Connected {
                        session_id: new_session_id,
                        version: PROTOCOL_VERSION,
                    }
                }
            }

            Request::OpenPath { path, flags } => {
                match self.fs.borrow_mut().open_path_with_flags(&path, flags) {
                    Ok(fd) => {
                        // Track fd -> path mapping for sync
                        self.fd_path_map.borrow_mut().insert(fd, path);
                        Response::Fd { fd }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Read { fd, length } => {
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
                        // Enqueue upload with actual path
                        if let Some(ref sync) = self.sync_manager {
                            if let Some(path) = self.fd_path_map.borrow().get(&fd) {
                                sync.enqueue_upload(path.clone());
                            }
                        }
                        Response::Written { count: n }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Close { fd } => {
                // Remove fd -> path mapping
                self.fd_path_map.borrow_mut().remove(&fd);
                match self.fs.borrow_mut().close(fd) {
                    Ok(()) => Response::Ok,
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Seek { fd, offset, whence } => {
                match self.fs.borrow_mut().seek(fd, offset, whence) {
                    Ok(pos) => Response::Position { pos },
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Ftruncate { fd, size } => match self.fs.borrow_mut().ftruncate(fd, size) {
                Ok(()) => {
                    // Enqueue upload with actual path
                    if let Some(ref sync) = self.sync_manager {
                        if let Some(path) = self.fd_path_map.borrow().get(&fd) {
                            sync.enqueue_upload(path.clone());
                        }
                    }
                    Response::Ok
                }
                Err(e) => map_fs_error(e),
            },

            Request::Fstat { fd } => match self.fs.borrow().fstat(fd) {
                Ok(meta) => Response::Metadata {
                    metadata: Metadata {
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
                    metadata: Metadata {
                        size: meta.size,
                        created: meta.created,
                        modified: meta.modified,
                        is_dir: meta.is_dir,
                    },
                },
                Err(e) => map_fs_error(e),
            },

            Request::Mkdir { path } => match self.fs.borrow_mut().mkdir(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::MkdirP { path } => match self.fs.borrow_mut().mkdir_p(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::Unlink { path } => {
                let result = self.fs.borrow_mut().unlink(&path);
                match result {
                    Ok(()) => {
                        // Enqueue delete for S3 sync
                        if let Some(ref sync) = self.sync_manager {
                            sync.enqueue_delete(path);
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

            Request::Rmdir { path } => match self.fs.borrow_mut().rmdir(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::OpenAt {
                dir_fd,
                path,
                flags,
            } => match self.fs.borrow_mut().open_at(dir_fd, &path, flags) {
                Ok(fd) => {
                    // Compute absolute path for sync tracking
                    if let Some(dir_path) = self.fd_path_map.borrow().get(&dir_fd) {
                        let abs_path = if dir_path == "/" {
                            format!("/{}", path.trim_start_matches('/'))
                        } else {
                            format!(
                                "{}/{}",
                                dir_path.trim_end_matches('/'),
                                path.trim_start_matches('/')
                            )
                        };
                        self.fd_path_map.borrow_mut().insert(fd, abs_path);
                    }
                    Response::Fd { fd }
                }
                Err(e) => map_fs_error(e),
            },
        }
    }
}

/// Map fs-core error to RPC error response
fn map_fs_error(error: FsError) -> Response {
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

/// Try to read a message with timeout
/// Returns Timeout if no data arrives within POLL_TIMEOUT_NS
fn try_read_message(stream: &InputStream) -> ReadResult {
    // First, wait for initial data with timeout
    let stream_pollable = stream.subscribe();
    let timeout_pollable = subscribe_duration(POLL_TIMEOUT_NS);

    // Poll both: stream readiness and timeout
    let ready = poll(&[&stream_pollable, &timeout_pollable]);

    // Check which pollable is ready
    // ready[0] = stream, ready[1] = timeout
    if ready.is_empty() || (ready.len() == 1 && ready[0] == 1) {
        // Only timeout fired, no data
        return ReadResult::Timeout;
    }

    // Stream has data (or is closed), try to read
    match stream.read(1) {
        Ok(bytes) if bytes.is_empty() => {
            // Stream closed
            ReadResult::Disconnected
        }
        Ok(first_byte) => {
            // Got first byte, now read the rest of the length prefix
            let mut len_buf = first_byte.to_vec();

            // Read remaining 3 bytes of length prefix (blocking is ok now, data is coming)
            while len_buf.len() < 4 {
                match stream.blocking_read(4 - len_buf.len() as u64) {
                    Ok(bytes) => {
                        if bytes.is_empty() {
                            let pollable = stream.subscribe();
                            poll(&[&pollable]);
                            continue;
                        }
                        len_buf.extend_from_slice(&bytes);
                    }
                    Err(e) => {
                        if matches!(e, wasi::io::streams::StreamError::Closed) {
                            return ReadResult::Disconnected;
                        }
                        let pollable = stream.subscribe();
                        poll(&[&pollable]);
                        continue;
                    }
                }
            }

            let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;

            // Read message body
            let mut data = Vec::new();
            while (data.len() as u64) < len {
                match stream.blocking_read(len - data.len() as u64) {
                    Ok(bytes) => {
                        if bytes.is_empty() {
                            let pollable = stream.subscribe();
                            poll(&[&pollable]);
                            continue;
                        }
                        data.extend_from_slice(&bytes);
                    }
                    Err(e) => {
                        if matches!(e, wasi::io::streams::StreamError::Closed) {
                            return ReadResult::Disconnected;
                        }
                        let pollable = stream.subscribe();
                        poll(&[&pollable]);
                        continue;
                    }
                }
            }

            ReadResult::Message(data)
        }
        Err(wasi::io::streams::StreamError::Closed) => ReadResult::Disconnected,
        Err(_) => {
            // Other error, treat as timeout to retry
            ReadResult::Timeout
        }
    }
}

/// Write a length-prefixed message to stream
fn write_message(stream: &OutputStream, data: &[u8]) -> bool {
    let len = data.len() as u32;
    let len_bytes = len.to_be_bytes();

    loop {
        match stream.blocking_write_and_flush(&len_bytes) {
            Ok(()) => break,
            Err(_) => {
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }

    loop {
        match stream.blocking_write_and_flush(data) {
            Ok(()) => return true,
            Err(_) => {
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }
}

/// Handle a single client connection
async fn handle_client(ctx: &ServerContext, input: InputStream, output: OutputStream) {
    println!("Client connected");

    loop {
        // Try to read with timeout for periodic sync
        let request_bytes = match try_read_message(&input) {
            ReadResult::Message(bytes) => bytes,
            ReadResult::Timeout => {
                // No request received, but run sync check
                if let Some(ref sync) = ctx.sync_manager {
                    sync.maybe_sync().await;
                }
                continue;
            }
            ReadResult::Disconnected => {
                println!("Client disconnected");
                // Flush pending operations on client disconnect
                if let Some(ref sync) = ctx.sync_manager {
                    if sync.pending_count() > 0 {
                        println!("[sync] Flushing pending operations on client disconnect...");
                        if let Err(e) = sync.force_flush().await {
                            eprintln!("[sync] Failed to flush: {}", e);
                        }
                    }
                }
                return;
            }
        };

        // Parse request JSON
        let rpc_request: RpcRequest = match serde_json::from_slice(&request_bytes) {
            Ok(req) => req,
            Err(e) => {
                eprintln!("Failed to parse request: {}", e);
                let response = Response::Error {
                    code: ErrorCode::SerializationError,
                    message: "Failed to parse request JSON".to_string(),
                };
                if let Ok(response_bytes) = serde_json::to_vec(&response) {
                    write_message(&output, &response_bytes);
                }
                continue;
            }
        };

        // Handle request (synchronous)
        let response = ctx.handle_request(rpc_request.request, rpc_request.session_id);

        // Serialize and send response
        let response_bytes = match serde_json::to_vec(&response) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Failed to serialize response: {}", e);
                continue;
            }
        };

        if !write_message(&output, &response_bytes) {
            println!("Client disconnected (write error)");
            // Flush pending operations on client disconnect
            if let Some(ref sync) = ctx.sync_manager {
                if sync.pending_count() > 0 {
                    println!("[sync] Flushing pending operations on client disconnect...");
                    if let Err(e) = sync.force_flush().await {
                        eprintln!("[sync] Failed to flush: {}", e);
                    }
                }
            }
            return;
        }

        // Check if we need to sync to S3 (based on dirty count threshold)
        if let Some(ref sync) = ctx.sync_manager {
            sync.maybe_sync().await;
        }
    }
}

/// Initialize server with optional S3 persistence
async fn init_server() -> ServerContext {
    // Check for S3 configuration via environment variables
    let s3_bucket = std::env::var("VFS_S3_BUCKET").ok();
    let s3_prefix = std::env::var("VFS_S3_PREFIX").unwrap_or_else(|_| "vfs/".to_string());

    let (fs, sync_manager) = if let Some(bucket) = s3_bucket {
        println!(
            "S3 persistence enabled: bucket={}, prefix={}",
            bucket, s3_prefix
        );

        // Initialize S3 client
        let s3 = Rc::new(S3Storage::new(bucket, s3_prefix).await);

        // Try to load existing files from S3
        let (fs, metadata_cache) = match init_from_s3(&s3).await {
            Ok((fs, cache)) => (fs, cache),
            Err(e) => {
                eprintln!(
                    "Failed to load from S3: {}, starting with empty filesystem",
                    e
                );
                (Fs::new(), MetadataCache::new())
            }
        };

        let fs = Rc::new(RefCell::new(fs));

        // Create sync manager
        let config = SyncConfig::default();
        let sync_manager = SyncManager::new(s3, fs.clone(), metadata_cache, config);

        (fs, Some(sync_manager))
    } else {
        println!("S3 persistence disabled (VFS_S3_BUCKET not set)");
        (Rc::new(RefCell::new(Fs::new())), None)
    };

    ServerContext {
        fs,
        sync_manager,
        fd_path_map: RefCell::new(HashMap::new()),
    }
}

/// Main entry point
#[no_mangle]
pub extern "C" fn _start() {
    // Use single-threaded tokio runtime for WASI
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
        .block_on(async_main());
}

async fn async_main() {
    println!("VFS RPC Server starting...");

    // Initialize server with optional S3 persistence
    let ctx = init_server().await;

    // Create TCP socket
    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4).expect("Failed to create TCP socket");

    // Bind to localhost:9000
    let bind_addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port: 9000,
        address: (127, 0, 0, 1),
    });

    socket
        .start_bind(&network, bind_addr)
        .expect("Failed to start bind");
    socket.finish_bind().expect("Failed to finish bind");

    println!("Socket bound to 127.0.0.1:9000");

    // Start listening
    socket.start_listen().expect("Failed to start listen");
    socket.finish_listen().expect("Failed to finish listen");

    println!("VFS RPC Server listening on 127.0.0.1:9000");
    println!("Protocol version: {}", PROTOCOL_VERSION);
    println!("Waiting for connections...");

    // Accept loop
    loop {
        let (_client_socket, input_stream, output_stream) = loop {
            match socket.accept() {
                Ok(result) => break result,
                Err(e) => match e {
                    wasi::sockets::network::ErrorCode::WouldBlock => {
                        let pollable = socket.subscribe();
                        poll(&[&pollable]);
                        continue;
                    }
                    _ => {
                        eprintln!("Failed to accept connection: {:?}", e);
                        continue;
                    }
                },
            }
        };

        // Handle client
        handle_client(&ctx, input_stream, output_stream).await;
    }
}
