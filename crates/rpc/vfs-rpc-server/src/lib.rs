//! VFS RPC Server
//!
//! A WebAssembly component that exposes fs-core filesystem over TCP sockets.
//! Multiple clients can connect and share the same in-memory filesystem.

#![no_main]
#![allow(warnings)]

use std::cell::RefCell;

use fs_core::{Fs, FsError};
use vfs_rpc_protocol::{DirEntry, ErrorCode, Metadata, Request, Response, PROTOCOL_VERSION};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "vfs-rpc-server",
    path: "../../../wit",
    generate_all,
});

use wasi::io::poll::poll;
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

// Global filesystem instance
static mut VFS: Option<RefCell<Fs>> = None;

/// Initialize the VFS
fn init_vfs() {
    unsafe {
        if VFS.is_none() {
            VFS = Some(RefCell::new(Fs::new()));
            println!("VFS initialized");
        }
    }
}

/// Handle a single RPC request
fn handle_request(request: Request) -> Response {
    unsafe {
        let vfs_ref = match VFS.as_ref() {
            Some(vfs) => vfs,
            None => {
                return Response::Error {
                    code: ErrorCode::ProtocolError,
                    message: "VFS not initialized".to_string(),
                }
            }
        };

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
                    Response::Connected {
                        session_id: 1,
                        version: PROTOCOL_VERSION,
                    }
                }
            }

            Request::OpenPath { path, flags } => {
                match vfs_ref.borrow_mut().open_path_with_flags(&path, flags) {
                    Ok(fd) => Response::Fd { fd },
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Read { fd, length } => {
                let mut buf = vec![0u8; length];
                match vfs_ref.borrow_mut().read(fd, &mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Response::Data { bytes: buf }
                    }
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Write { fd, data } => match vfs_ref.borrow_mut().write(fd, &data) {
                Ok(n) => Response::Written { count: n },
                Err(e) => map_fs_error(e),
            },

            Request::Close { fd } => match vfs_ref.borrow_mut().close(fd) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::Seek { fd, offset, whence } => {
                match vfs_ref.borrow_mut().seek(fd, offset, whence) {
                    Ok(pos) => Response::Position { pos },
                    Err(e) => map_fs_error(e),
                }
            }

            Request::Ftruncate { fd, size } => match vfs_ref.borrow_mut().ftruncate(fd, size) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::Fstat { fd } => match vfs_ref.borrow().fstat(fd) {
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

            Request::Stat { path } => match vfs_ref.borrow().stat(&path) {
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

            Request::Mkdir { path } => match vfs_ref.borrow_mut().mkdir(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::MkdirP { path } => match vfs_ref.borrow_mut().mkdir_p(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::Unlink { path } => match vfs_ref.borrow_mut().unlink(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::Readdir { path } => match vfs_ref.borrow().readdir(&path) {
                Ok(names) => {
                    // readdir returns Vec<String>, we need to convert to Vec<DirEntry>
                    // Since readdir doesn't provide is_dir info, we'll need to stat each entry
                    let mut entries = Vec::new();
                    for name in names {
                        let full_path = if path == "/" {
                            format!("/{}", name)
                        } else {
                            format!("{}/{}", path, name)
                        };
                        let is_dir = vfs_ref
                            .borrow()
                            .stat(&full_path)
                            .map(|meta| meta.is_dir)
                            .unwrap_or(false);
                        entries.push(DirEntry { name, is_dir });
                    }
                    Response::DirEntries { entries }
                }
                Err(e) => map_fs_error(e),
            },

            Request::ReaddirFd { fd } => match vfs_ref.borrow().readdir_fd(fd) {
                Ok(entries) => {
                    // readdir_fd returns Vec<(String, bool)>
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

            Request::Rmdir { path } => match vfs_ref.borrow_mut().rmdir(&path) {
                Ok(()) => Response::Ok,
                Err(e) => map_fs_error(e),
            },

            Request::OpenAt {
                dir_fd,
                path,
                flags,
            } => match vfs_ref.borrow_mut().open_at(dir_fd, &path, flags) {
                Ok(fd) => Response::Fd { fd },
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

/// Read a length-prefixed message from stream
fn read_message(stream: &InputStream) -> Option<Vec<u8>> {
    // Read 4-byte length prefix with retry on would-block and partial reads
    println!("read_message: reading length prefix...");
    let mut len_buf = Vec::new();
    let mut empty_reads = 0;
    while len_buf.len() < 4 {
        match stream.blocking_read(4 - len_buf.len() as u64) {
            Ok(bytes) => {
                println!("read_message: got {} bytes for length prefix", bytes.len());
                if bytes.is_empty() {
                    empty_reads += 1;
                    if empty_reads > 10 {
                        // Too many empty reads, likely EOF
                        println!(
                            "read_message: EOF on length prefix after {} empty reads",
                            empty_reads
                        );
                        return None;
                    }
                    // Poll and retry
                    println!("read_message: empty read on length prefix, polling...");
                    let pollable = stream.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                empty_reads = 0; // Reset counter when we get data
                len_buf.extend_from_slice(&bytes);
            }
            Err(e) => {
                // Check if stream is closed
                if matches!(e, wasi::io::streams::StreamError::Closed) {
                    println!("read_message: stream closed");
                    return None;
                }
                println!("read_message: error reading length prefix: {:?}", e);
                // Wait for stream to be ready
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }

    let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;
    println!("read_message: message length is {} bytes", len);

    // Read message body with retry on would-block and partial reads
    let mut data = Vec::new();
    while (data.len() as u64) < len {
        println!(
            "read_message: reading body, have {} of {} bytes...",
            data.len(),
            len
        );
        match stream.blocking_read(len - data.len() as u64) {
            Ok(bytes) => {
                println!("read_message: got {} bytes for body", bytes.len());
                if bytes.is_empty() {
                    // Empty read doesn't necessarily mean EOF. Data might not have arrived yet
                    // Poll and retry
                    println!("read_message: empty read, polling...");
                    let pollable = stream.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                data.extend_from_slice(&bytes);
            }
            Err(e) => {
                println!("read_message: error reading body: {:?}", e);
                // Wait for stream to be ready
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }

    println!("read_message: successfully read complete message");
    Some(data)
}

/// Write a length-prefixed message to stream
fn write_message(stream: &OutputStream, data: &[u8]) -> bool {
    // Write length prefix with retry on would-block
    let len = data.len() as u32;
    let len_bytes = len.to_be_bytes();

    loop {
        match stream.blocking_write_and_flush(&len_bytes) {
            Ok(()) => break,
            Err(_) => {
                // Wait for stream to be ready
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }

    // Write message body with retry on would-block
    loop {
        match stream.blocking_write_and_flush(data) {
            Ok(()) => return true,
            Err(_) => {
                // Wait for stream to be ready
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }
}

/// Handle a single client connection
fn handle_client(input: InputStream, output: OutputStream) {
    println!("Client connected");

    loop {
        // Read request
        println!("Waiting for client message...");
        let request_bytes = match read_message(&input) {
            Some(bytes) => {
                println!("Received message: {} bytes", bytes.len());
                bytes
            }
            None => {
                println!("Client disconnected (read error)");
                return;
            }
        };

        // Parse request JSON
        let request: Request = match serde_json::from_slice(&request_bytes) {
            Ok(req) => req,
            Err(e) => {
                eprintln!("Failed to parse request: {}", e);
                // Send error response
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

        // Handle request
        let response = handle_request(request);

        // Serialize response
        let response_bytes = match serde_json::to_vec(&response) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Failed to serialize response: {}", e);
                continue;
            }
        };

        // Send response
        if !write_message(&output, &response_bytes) {
            println!("Client disconnected (write error)");
            return;
        }
    }
}

/// Main entry point
#[no_mangle]
pub extern "C" fn _start() {
    println!("VFS RPC Server starting...");

    // Initialize VFS
    init_vfs();

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
        // Try to accept connection
        let (_client_socket, input_stream, output_stream) = loop {
            match socket.accept() {
                Ok(result) => break result,
                Err(e) => {
                    // Check if it's a would-block error
                    match e {
                        wasi::sockets::network::ErrorCode::WouldBlock => {
                            // Need to wait for the socket to be ready
                            let pollable = socket.subscribe();
                            poll(&[&pollable]);
                            continue;
                        }
                        _ => {
                            eprintln!("Failed to accept connection: {:?}", e);
                            continue;
                        }
                    }
                }
            }
        };

        // Handle client (blocking, single-threaded for MVP)
        handle_client(input_stream, output_stream);
    }
}
