//! VFS RPC Protocol
//!
//! Defines message types and serialization for VFS RPC communication
//! between client and server over TCP sockets.
//!
//! Uses Protocol Buffers for efficient binary serialization.

use prost::Message;

// Include the generated protobuf code
pub mod vfs {
    include!(concat!(env!("OUT_DIR"), "/vfs.rs"));
}

pub use vfs::*;

/// Protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Default server port
pub const DEFAULT_PORT: u16 = 9000;

/// Error codes matching fs-core FsError
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
    NotFound = 1,
    NotADirectory = 2,
    IsADirectory = 3,
    InvalidArgument = 4,
    BadFileDescriptor = 5,
    PermissionDenied = 6,
    AlreadyExists = 7,
    NotEmpty = 8,
    NetworkError = 9,
    ProtocolError = 10,
    SerializationError = 11,
    Io = 12,
}

impl ErrorCode {
    /// Convert error code to human-readable string
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::NotFound => "Not found",
            ErrorCode::NotADirectory => "Not a directory",
            ErrorCode::IsADirectory => "Is a directory",
            ErrorCode::InvalidArgument => "Invalid argument",
            ErrorCode::BadFileDescriptor => "Bad file descriptor",
            ErrorCode::PermissionDenied => "Permission denied",
            ErrorCode::AlreadyExists => "Already exists",
            ErrorCode::NotEmpty => "Directory not empty",
            ErrorCode::NetworkError => "Network error",
            ErrorCode::ProtocolError => "Protocol error",
            ErrorCode::SerializationError => "Serialization error",
            ErrorCode::Io => "I/O error",
        }
    }

    /// Convert from i32 (protobuf representation)
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            1 => Some(ErrorCode::NotFound),
            2 => Some(ErrorCode::NotADirectory),
            3 => Some(ErrorCode::IsADirectory),
            4 => Some(ErrorCode::InvalidArgument),
            5 => Some(ErrorCode::BadFileDescriptor),
            6 => Some(ErrorCode::PermissionDenied),
            7 => Some(ErrorCode::AlreadyExists),
            8 => Some(ErrorCode::NotEmpty),
            9 => Some(ErrorCode::NetworkError),
            10 => Some(ErrorCode::ProtocolError),
            11 => Some(ErrorCode::SerializationError),
            12 => Some(ErrorCode::Io),
            _ => None,
        }
    }
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Shared internal types
// ---------------------------------------------------------------------------

/// Internal request enum
#[derive(Debug, Clone)]
pub enum Request {
    Connect {
        version: u32,
    },
    OpenPath {
        path: String,
        flags: u32,
    },
    OpenAt {
        dir_fd: u32,
        path: String,
        flags: u32,
    },
    Read {
        fd: u32,
        length: usize,
    },
    Write {
        fd: u32,
        data: Vec<u8>,
    },
    Close {
        fd: u32,
    },
    Seek {
        fd: u32,
        offset: i64,
        whence: i32,
    },
    Ftruncate {
        fd: u32,
        size: u64,
    },
    Stat {
        path: String,
    },
    Fstat {
        fd: u32,
    },
    Mkdir {
        path: String,
    },
    MkdirP {
        path: String,
    },
    Unlink {
        path: String,
    },
    Readdir {
        path: String,
    },
    ReaddirFd {
        fd: u32,
    },
    Rmdir {
        path: String,
    },
    AppendWrite {
        fd: u32,
        data: Vec<u8>,
    },
    Rename {
        old_path: String,
        new_path: String,
    },
}

/// Internal response enum
#[derive(Debug, Clone)]
pub enum Response {
    Connected { session_id: String, version: u32 },
    Ok,
    Fd { fd: u32 },
    Data { bytes: Vec<u8> },
    Written { count: usize },
    Position { pos: u64 },
    Metadata { metadata: FileMetadata },
    DirEntries { entries: Vec<DirEntry> },
    Error { code: ErrorCode, message: String },
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// RPC request with session tracking
#[derive(Debug, Clone)]
pub struct RpcRequestMessage {
    pub session_id: Option<String>,
    pub request: Request,
}

// ---------------------------------------------------------------------------
// Conversion: protobuf → internal (server side)
// ---------------------------------------------------------------------------

/// Convert protobuf RpcRequest to internal RpcRequestMessage
pub fn from_proto_request(proto_req: vfs::RpcRequest) -> Result<RpcRequestMessage, &'static str> {
    use rpc_request::Request as R;

    let session_id = proto_req.session_id;
    let request = match proto_req.request {
        Some(R::Connect(c)) => Request::Connect { version: c.version },
        Some(R::OpenPath(o)) => Request::OpenPath {
            path: o.path,
            flags: o.flags,
        },
        Some(R::OpenAt(o)) => Request::OpenAt {
            dir_fd: o.dir_fd,
            path: o.path,
            flags: o.flags,
        },
        Some(R::Read(r)) => Request::Read {
            fd: r.fd,
            length: r.length as usize,
        },
        Some(R::Write(w)) => Request::Write {
            fd: w.fd,
            data: w.data,
        },
        Some(R::Close(c)) => Request::Close { fd: c.fd },
        Some(R::Seek(s)) => Request::Seek {
            fd: s.fd,
            offset: s.offset,
            whence: s.whence,
        },
        Some(R::Ftruncate(f)) => Request::Ftruncate {
            fd: f.fd,
            size: f.size,
        },
        Some(R::Stat(s)) => Request::Stat { path: s.path },
        Some(R::Fstat(f)) => Request::Fstat { fd: f.fd },
        Some(R::Mkdir(m)) => Request::Mkdir { path: m.path },
        Some(R::MkdirP(m)) => Request::MkdirP { path: m.path },
        Some(R::Unlink(u)) => Request::Unlink { path: u.path },
        Some(R::Readdir(r)) => Request::Readdir { path: r.path },
        Some(R::ReaddirFd(r)) => Request::ReaddirFd { fd: r.fd },
        Some(R::Rmdir(r)) => Request::Rmdir { path: r.path },
        Some(R::AppendWrite(a)) => Request::AppendWrite {
            fd: a.fd,
            data: a.data,
        },
        Some(R::Rename(r)) => Request::Rename {
            old_path: r.old_path,
            new_path: r.new_path,
        },
        None => return Err("Missing request"),
    };
    Ok(RpcRequestMessage {
        session_id,
        request,
    })
}

/// Convert internal Response to protobuf RpcResponse
pub fn to_proto_response(response: Response) -> vfs::RpcResponse {
    use rpc_response::Response as R;

    let response = match response {
        Response::Connected {
            session_id,
            version,
        } => R::Connected(vfs::Connected {
            session_id,
            version,
        }),
        Response::Ok => R::Ok(vfs::Ok {}),
        Response::Fd { fd } => R::Fd(vfs::Fd { fd }),
        Response::Data { bytes } => R::Data(vfs::Data { bytes }),
        Response::Written { count } => R::Written(vfs::Written {
            count: count as u64,
        }),
        Response::Position { pos } => R::Position(vfs::Position { pos }),
        Response::Metadata { metadata } => R::Metadata(vfs::MetadataResponse {
            size: metadata.size,
            created: metadata.created,
            modified: metadata.modified,
            is_dir: metadata.is_dir,
        }),
        Response::DirEntries { entries } => R::DirEntries(vfs::DirEntries {
            entries: entries
                .into_iter()
                .map(|e| vfs::DirEntry {
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect(),
        }),
        Response::Error { code, message } => R::Error(vfs::Error {
            code: code as i32,
            message,
        }),
    };
    vfs::RpcResponse {
        response: Some(response),
    }
}

// ---------------------------------------------------------------------------
// Conversion: internal → bytes (client/adapter side)
// ---------------------------------------------------------------------------

/// Convert internal RpcRequestMessage to protobuf-encoded bytes
pub fn to_proto_request_bytes(rpc_request: &RpcRequestMessage) -> Vec<u8> {
    use rpc_request::Request as R;

    let request = match &rpc_request.request {
        Request::Connect { version } => R::Connect(vfs::Connect { version: *version }),
        Request::OpenPath { path, flags } => R::OpenPath(vfs::OpenPath {
            path: path.clone(),
            flags: *flags,
        }),
        Request::OpenAt {
            dir_fd,
            path,
            flags,
        } => R::OpenAt(vfs::OpenAt {
            dir_fd: *dir_fd,
            path: path.clone(),
            flags: *flags,
        }),
        Request::Read { fd, length } => R::Read(vfs::Read {
            fd: *fd,
            length: *length as u64,
        }),
        Request::Write { fd, data } => R::Write(vfs::Write {
            fd: *fd,
            data: data.clone(),
        }),
        Request::Close { fd } => R::Close(vfs::Close { fd: *fd }),
        Request::Seek { fd, offset, whence } => R::Seek(vfs::Seek {
            fd: *fd,
            offset: *offset,
            whence: *whence,
        }),
        Request::Ftruncate { fd, size } => R::Ftruncate(vfs::Ftruncate {
            fd: *fd,
            size: *size,
        }),
        Request::Stat { path } => R::Stat(vfs::Stat { path: path.clone() }),
        Request::Fstat { fd } => R::Fstat(vfs::Fstat { fd: *fd }),
        Request::Mkdir { path } => R::Mkdir(vfs::Mkdir { path: path.clone() }),
        Request::MkdirP { path } => R::MkdirP(vfs::MkdirP { path: path.clone() }),
        Request::Unlink { path } => R::Unlink(vfs::Unlink { path: path.clone() }),
        Request::Readdir { path } => R::Readdir(vfs::Readdir { path: path.clone() }),
        Request::ReaddirFd { fd } => R::ReaddirFd(vfs::ReaddirFd { fd: *fd }),
        Request::Rmdir { path } => R::Rmdir(vfs::Rmdir { path: path.clone() }),
        Request::AppendWrite { fd, data } => R::AppendWrite(vfs::AppendWrite {
            fd: *fd,
            data: data.clone(),
        }),
        Request::Rename { old_path, new_path } => R::Rename(vfs::Rename {
            old_path: old_path.clone(),
            new_path: new_path.clone(),
        }),
    };

    let proto_request = vfs::RpcRequest {
        session_id: rpc_request.session_id.clone(),
        request: Some(request),
    };

    proto_request.encode_to_vec()
}

/// Convert protobuf-encoded bytes to internal Response
pub fn from_proto_response_bytes(data: &[u8]) -> Result<Response, ErrorCode> {
    use rpc_response::Response as R;

    let proto_response =
        vfs::RpcResponse::decode(data).map_err(|_| ErrorCode::SerializationError)?;

    match proto_response.response {
        Some(R::Connected(c)) => Ok(Response::Connected {
            session_id: c.session_id,
            version: c.version,
        }),
        Some(R::Ok(_)) => Ok(Response::Ok),
        Some(R::Fd(f)) => Ok(Response::Fd { fd: f.fd }),
        Some(R::Data(d)) => Ok(Response::Data { bytes: d.bytes }),
        Some(R::Written(w)) => Ok(Response::Written {
            count: w.count as usize,
        }),
        Some(R::Position(p)) => Ok(Response::Position { pos: p.pos }),
        Some(R::Metadata(m)) => Ok(Response::Metadata {
            metadata: FileMetadata {
                size: m.size,
                created: m.created,
                modified: m.modified,
                is_dir: m.is_dir,
            },
        }),
        Some(R::DirEntries(d)) => Ok(Response::DirEntries {
            entries: d
                .entries
                .into_iter()
                .map(|e| DirEntry {
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect(),
        }),
        Some(R::Error(e)) => Ok(Response::Error {
            code: ErrorCode::from_i32(e.code).unwrap_or(ErrorCode::Io),
            message: e.message,
        }),
        None => Err(ErrorCode::SerializationError),
    }
}
