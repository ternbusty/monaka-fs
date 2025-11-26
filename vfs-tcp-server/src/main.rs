//! VFS TCP Server (Native)
//!
//! A native Rust TCP server that exposes fs-core filesystem over TCP sockets.
//! Multiple WASM clients can connect and share the same in-memory filesystem.

use anyhow::{Context, Result};
use fs_core::{Fs, FsError};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use vfs_rpc_protocol::{DirEntry, ErrorCode, Metadata, Request, Response, PROTOCOL_VERSION};

fn main() -> Result<()> {
    println!("VFS TCP Server starting...");

    // Initialize VFS
    let vfs = Rc::new(RefCell::new(Fs::new()));
    println!("VFS initialized");

    // Bind to localhost:9000
    let listener =
        TcpListener::bind("127.0.0.1:9000").context("Failed to bind to 127.0.0.1:9000")?;

    println!("VFS TCP Server listening on 127.0.0.1:9000");
    println!("Protocol version: {}", PROTOCOL_VERSION);
    println!("Waiting for connections...");

    // Accept loop (single-threaded, handles one client at a time)
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(e) = handle_client(stream, Rc::clone(&vfs)) {
                    eprintln!("Client handler error: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
            }
        }
    }

    Ok(())
}

/// Handle a single client connection
fn handle_client(mut stream: TcpStream, vfs: Rc<RefCell<Fs>>) -> Result<()> {
    println!("Client connected: {:?}", stream.peer_addr()?);

    loop {
        // Read request
        let request_bytes = match read_message(&mut stream) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("Client disconnected: {}", e);
                return Ok(());
            }
        };

        // Parse request JSON
        let request: Request =
            serde_json::from_slice(&request_bytes).context("Failed to parse request JSON")?;

        // Handle request
        let response = handle_request(request, &vfs);

        // Serialize response
        let response_bytes =
            serde_json::to_vec(&response).context("Failed to serialize response")?;

        // Send response
        write_message(&mut stream, &response_bytes).context("Failed to write response")?;
    }
}

/// Handle a single RPC request
fn handle_request(request: Request, vfs: &Rc<RefCell<Fs>>) -> Response {
    let mut vfs = vfs.borrow_mut();

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

        Request::OpenPath { path, flags } => match vfs.open_path_with_flags(&path, flags) {
            Ok(fd) => Response::Fd { fd },
            Err(e) => map_fs_error(e),
        },

        Request::Read { fd, length } => {
            let mut buf = vec![0u8; length];
            match vfs.read(fd, &mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    Response::Data { bytes: buf }
                }
                Err(e) => map_fs_error(e),
            }
        }

        Request::Write { fd, data } => match vfs.write(fd, &data) {
            Ok(n) => Response::Written { count: n },
            Err(e) => map_fs_error(e),
        },

        Request::Close { fd } => match vfs.close(fd) {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        },

        Request::Seek { fd, offset, whence } => match vfs.seek(fd, offset, whence) {
            Ok(pos) => Response::Position { pos },
            Err(e) => map_fs_error(e),
        },

        Request::Ftruncate { fd, size } => match vfs.ftruncate(fd, size) {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        },

        Request::Fstat { fd } => match vfs.fstat(fd) {
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

        Request::Stat { path } => match vfs.stat(&path) {
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

        Request::Mkdir { path } => match vfs.mkdir(&path) {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        },

        Request::MkdirP { path } => match vfs.mkdir_p(&path) {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        },

        Request::Unlink { path } => match vfs.unlink(&path) {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        },

        Request::Readdir { path } => match vfs.readdir(&path) {
            Ok(names) => {
                let mut entries = Vec::new();
                for name in names {
                    let full_path = if path == "/" {
                        format!("/{}", name)
                    } else {
                        format!("{}/{}", path, name)
                    };
                    let is_dir = vfs
                        .stat(&full_path)
                        .map(|meta| meta.is_dir)
                        .unwrap_or(false);
                    entries.push(DirEntry { name, is_dir });
                }
                Response::DirEntries { entries }
            }
            Err(e) => map_fs_error(e),
        },

        Request::ReaddirFd { fd } => match vfs.readdir_fd(fd) {
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

        Request::Rmdir { path } => match vfs.rmdir(&path) {
            Ok(()) => Response::Ok,
            Err(e) => map_fs_error(e),
        },

        Request::OpenAt {
            dir_fd,
            path,
            flags,
        } => match vfs.open_at(dir_fd, &path, flags) {
            Ok(fd) => Response::Fd { fd },
            Err(e) => map_fs_error(e),
        },
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
fn read_message(stream: &mut TcpStream) -> Result<Vec<u8>> {
    // Read 4-byte length prefix
    let mut len_bytes = [0u8; 4];
    stream
        .read_exact(&mut len_bytes)
        .context("Failed to read length prefix")?;

    let len = u32::from_be_bytes(len_bytes) as usize;

    // Read message body
    let mut data = vec![0u8; len];
    stream
        .read_exact(&mut data)
        .context("Failed to read message body")?;

    Ok(data)
}

/// Write a length-prefixed message to stream
fn write_message(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    // Write length prefix
    let len = data.len() as u32;
    let len_bytes = len.to_be_bytes();
    stream
        .write_all(&len_bytes)
        .context("Failed to write length prefix")?;

    // Write message body
    stream
        .write_all(data)
        .context("Failed to write message body")?;

    stream.flush().context("Failed to flush stream")?;

    Ok(())
}
