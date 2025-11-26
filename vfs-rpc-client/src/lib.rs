//! VFS RPC Client
//!
//! A library that provides filesystem operations by connecting to a remote VFS RPC server over TCP.
//! This library is meant to be used inside WASM components that use wit_bindgen.

#![allow(warnings)]

use std::cell::RefCell;
use vfs_rpc_protocol::{DirEntry, ErrorCode, Metadata, Request, Response, PROTOCOL_VERSION};

// Re-export types that applications will need
pub use vfs_rpc_protocol;

// Type aliases for WASI types - applications using this library must import these from their wit_bindgen
pub type InputStream = *mut (); // Placeholder - will be replaced by real type in application
pub type OutputStream = *mut (); // Placeholder - will be replaced by real type in application

/// VFS client error type
#[derive(Debug)]
pub enum VfsClientError {
    /// Connection failed
    ConnectionFailed(String),
    /// Network I/O error
    IoError(String),
    /// Protocol error (serialization/deserialization)
    ProtocolError(String),
    /// Server returned error
    ServerError { code: ErrorCode, message: String },
}

impl std::fmt::Display for VfsClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VfsClientError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            VfsClientError::IoError(msg) => write!(f, "I/O error: {}", msg),
            VfsClientError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            VfsClientError::ServerError { code, message } => {
                write!(f, "Server error {}: {}", code, message)
            }
        }
    }
}

impl std::error::Error for VfsClientError {}

/// VFS RPC Client
pub struct VfsClient {
    input: RefCell<InputStream>,
    output: RefCell<OutputStream>,
    session_id: u64,
}

impl VfsClient {
    /// Connect to VFS server at the given address
    pub fn connect(host: &str, port: u16) -> Result<Self, VfsClientError> {
        // Create TCP socket
        let network = instance_network();
        let socket = create_tcp_socket(IpAddressFamily::Ipv4).map_err(|e| {
            VfsClientError::ConnectionFailed(format!("Failed to create socket: {:?}", e))
        })?;

        // Parse host address (assume IPv4 for now)
        let addr = if host == "localhost" || host == "127.0.0.1" {
            IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: (127, 0, 0, 1),
            })
        } else {
            return Err(VfsClientError::ConnectionFailed(
                "Only localhost is supported".to_string(),
            ));
        };

        // Connect to server
        socket.start_connect(&network, addr).map_err(|e| {
            VfsClientError::ConnectionFailed(format!("Failed to start connect: {:?}", e))
        })?;

        let (input, output) = socket.finish_connect().map_err(|e| {
            VfsClientError::ConnectionFailed(format!("Failed to finish connect: {:?}", e))
        })?;

        // Send Connect request
        let connect_req = Request::Connect {
            version: PROTOCOL_VERSION,
        };

        let mut client = VfsClient {
            input: RefCell::new(input),
            output: RefCell::new(output),
            session_id: 0,
        };

        let response = client.send_request(connect_req)?;

        match response {
            Response::Connected {
                session_id,
                version,
            } => {
                if version != PROTOCOL_VERSION {
                    return Err(VfsClientError::ProtocolError(format!(
                        "Protocol version mismatch: server={}, client={}",
                        version, PROTOCOL_VERSION
                    )));
                }
                client.session_id = session_id;
                Ok(client)
            }
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Connect".to_string(),
            )),
        }
    }

    /// Send a request and receive response
    fn send_request(&self, request: Request) -> Result<Response, VfsClientError> {
        // Serialize request
        let request_bytes = serde_json::to_vec(&request).map_err(|e| {
            VfsClientError::ProtocolError(format!("Failed to serialize request: {}", e))
        })?;

        // Send length-prefixed message
        self.write_message(&request_bytes)?;

        // Read response
        let response_bytes = self.read_message()?;

        // Deserialize response
        let response: Response = serde_json::from_slice(&response_bytes).map_err(|e| {
            VfsClientError::ProtocolError(format!("Failed to deserialize response: {}", e))
        })?;

        Ok(response)
    }

    /// Write length-prefixed message
    fn write_message(&self, data: &[u8]) -> Result<(), VfsClientError> {
        let output = self.output.borrow();

        // Write length prefix
        let len = data.len() as u32;
        let len_bytes = len.to_be_bytes();

        output
            .blocking_write_and_flush(&len_bytes)
            .map_err(|e| VfsClientError::IoError(format!("Failed to write length: {:?}", e)))?;

        // Write message body
        output
            .blocking_write_and_flush(data)
            .map_err(|e| VfsClientError::IoError(format!("Failed to write data: {:?}", e)))?;

        Ok(())
    }

    /// Read length-prefixed message
    fn read_message(&self) -> Result<Vec<u8>, VfsClientError> {
        let input = self.input.borrow();

        // Read 4-byte length prefix
        let len_bytes = input
            .blocking_read(4)
            .map_err(|e| VfsClientError::IoError(format!("Failed to read length: {:?}", e)))?;

        if len_bytes.len() != 4 {
            return Err(VfsClientError::IoError(
                "Incomplete length prefix".to_string(),
            ));
        }

        let len =
            u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as u64;

        // Read message body
        let data = input
            .blocking_read(len)
            .map_err(|e| VfsClientError::IoError(format!("Failed to read data: {:?}", e)))?;

        if data.len() as u64 != len {
            return Err(VfsClientError::IoError("Incomplete message".to_string()));
        }

        Ok(data)
    }

    /// Open file at path with flags
    pub fn open_path(&self, path: &str, flags: u32) -> Result<u32, VfsClientError> {
        let request = Request::OpenPath {
            path: path.to_string(),
            flags,
        };

        match self.send_request(request)? {
            Response::Fd { fd } => Ok(fd),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to OpenPath".to_string(),
            )),
        }
    }

    /// Read from file descriptor
    pub fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, VfsClientError> {
        let request = Request::Read {
            fd,
            length: buf.len(),
        };

        match self.send_request(request)? {
            Response::Data { bytes } => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok(n)
            }
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Read".to_string(),
            )),
        }
    }

    /// Write to file descriptor
    pub fn write(&self, fd: u32, data: &[u8]) -> Result<usize, VfsClientError> {
        let request = Request::Write {
            fd,
            data: data.to_vec(),
        };

        match self.send_request(request)? {
            Response::Written { count } => Ok(count),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Write".to_string(),
            )),
        }
    }

    /// Close file descriptor
    pub fn close(&self, fd: u32) -> Result<(), VfsClientError> {
        let request = Request::Close { fd };

        match self.send_request(request)? {
            Response::Ok => Ok(()),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Close".to_string(),
            )),
        }
    }

    /// Seek in file
    pub fn seek(&self, fd: u32, offset: i64, whence: i32) -> Result<u64, VfsClientError> {
        let request = Request::Seek { fd, offset, whence };

        match self.send_request(request)? {
            Response::Position { pos } => Ok(pos),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Seek".to_string(),
            )),
        }
    }

    /// Truncate file to size
    pub fn ftruncate(&self, fd: u32, size: u64) -> Result<(), VfsClientError> {
        let request = Request::Ftruncate { fd, size };

        match self.send_request(request)? {
            Response::Ok => Ok(()),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Ftruncate".to_string(),
            )),
        }
    }

    /// Get file metadata by path
    pub fn stat(&self, path: &str) -> Result<Metadata, VfsClientError> {
        let request = Request::Stat {
            path: path.to_string(),
        };

        match self.send_request(request)? {
            Response::Metadata { metadata } => Ok(metadata),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Stat".to_string(),
            )),
        }
    }

    /// Get file metadata by file descriptor
    pub fn fstat(&self, fd: u32) -> Result<Metadata, VfsClientError> {
        let request = Request::Fstat { fd };

        match self.send_request(request)? {
            Response::Metadata { metadata } => Ok(metadata),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Fstat".to_string(),
            )),
        }
    }

    /// Create directory
    pub fn mkdir(&self, path: &str) -> Result<(), VfsClientError> {
        let request = Request::Mkdir {
            path: path.to_string(),
        };

        match self.send_request(request)? {
            Response::Ok => Ok(()),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Mkdir".to_string(),
            )),
        }
    }

    /// Create directory and all parent directories
    pub fn mkdir_p(&self, path: &str) -> Result<(), VfsClientError> {
        let request = Request::MkdirP {
            path: path.to_string(),
        };

        match self.send_request(request)? {
            Response::Ok => Ok(()),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to MkdirP".to_string(),
            )),
        }
    }

    /// Remove file
    pub fn unlink(&self, path: &str) -> Result<(), VfsClientError> {
        let request = Request::Unlink {
            path: path.to_string(),
        };

        match self.send_request(request)? {
            Response::Ok => Ok(()),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Unlink".to_string(),
            )),
        }
    }

    /// Read directory entries by path
    pub fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsClientError> {
        let request = Request::Readdir {
            path: path.to_string(),
        };

        match self.send_request(request)? {
            Response::DirEntries { entries } => Ok(entries),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Readdir".to_string(),
            )),
        }
    }

    /// Read directory entries by file descriptor
    pub fn readdir_fd(&self, fd: u32) -> Result<Vec<DirEntry>, VfsClientError> {
        let request = Request::ReaddirFd { fd };

        match self.send_request(request)? {
            Response::DirEntries { entries } => Ok(entries),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to ReaddirFd".to_string(),
            )),
        }
    }

    /// Remove empty directory
    pub fn rmdir(&self, path: &str) -> Result<(), VfsClientError> {
        let request = Request::Rmdir {
            path: path.to_string(),
        };

        match self.send_request(request)? {
            Response::Ok => Ok(()),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to Rmdir".to_string(),
            )),
        }
    }

    /// Open file relative to directory file descriptor
    pub fn open_at(&self, dir_fd: u32, path: &str, flags: u32) -> Result<u32, VfsClientError> {
        let request = Request::OpenAt {
            dir_fd,
            path: path.to_string(),
            flags,
        };

        match self.send_request(request)? {
            Response::Fd { fd } => Ok(fd),
            Response::Error { code, message } => Err(VfsClientError::ServerError { code, message }),
            _ => Err(VfsClientError::ProtocolError(
                "Unexpected response to OpenAt".to_string(),
            )),
        }
    }

    /// Get session ID
    pub fn session_id(&self) -> u64 {
        self.session_id
    }
}
