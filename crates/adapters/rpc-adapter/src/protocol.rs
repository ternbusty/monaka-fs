//! Internal protocol types and conversion to/from protobuf

use prost::Message;
use vfs_rpc_protocol::{self as proto, rpc_request, rpc_response, PROTOCOL_VERSION};

/// Internal request enum (matches old JSON-based protocol)
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
}

/// Internal response enum (matches old JSON-based protocol)
#[derive(Debug, Clone)]
pub enum Response {
    Connected { session_id: String, version: u32 },
    Ok,
    Fd { fd: u32 },
    Data { bytes: Vec<u8> },
    Written { count: usize },
    Position { pos: u64 },
    Metadata { metadata: Metadata },
    DirEntries { entries: Vec<DirEntry> },
    Error { code: ErrorCode, message: String },
}

#[derive(Debug, Clone)]
pub struct Metadata {
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
            _ => None,
        }
    }
}

/// RPC request with session tracking
#[derive(Debug, Clone)]
pub struct RpcRequest {
    pub session_id: Option<String>,
    pub request: Request,
}

/// Convert internal RpcRequest to protobuf bytes
pub fn to_proto_request_bytes(rpc_request: &RpcRequest) -> Vec<u8> {
    let request = match &rpc_request.request {
        Request::Connect { version } => {
            rpc_request::Request::Connect(proto::Connect { version: *version })
        }
        Request::OpenPath { path, flags } => rpc_request::Request::OpenPath(proto::OpenPath {
            path: path.clone(),
            flags: *flags,
        }),
        Request::OpenAt {
            dir_fd,
            path,
            flags,
        } => rpc_request::Request::OpenAt(proto::OpenAt {
            dir_fd: *dir_fd,
            path: path.clone(),
            flags: *flags,
        }),
        Request::Read { fd, length } => rpc_request::Request::Read(proto::Read {
            fd: *fd,
            length: *length as u64,
        }),
        Request::Write { fd, data } => rpc_request::Request::Write(proto::Write {
            fd: *fd,
            data: data.clone(),
        }),
        Request::Close { fd } => rpc_request::Request::Close(proto::Close { fd: *fd }),
        Request::Seek { fd, offset, whence } => rpc_request::Request::Seek(proto::Seek {
            fd: *fd,
            offset: *offset,
            whence: *whence,
        }),
        Request::Ftruncate { fd, size } => rpc_request::Request::Ftruncate(proto::Ftruncate {
            fd: *fd,
            size: *size,
        }),
        Request::Stat { path } => rpc_request::Request::Stat(proto::Stat { path: path.clone() }),
        Request::Fstat { fd } => rpc_request::Request::Fstat(proto::Fstat { fd: *fd }),
        Request::Mkdir { path } => rpc_request::Request::Mkdir(proto::Mkdir { path: path.clone() }),
        Request::MkdirP { path } => {
            rpc_request::Request::MkdirP(proto::MkdirP { path: path.clone() })
        }
        Request::Unlink { path } => {
            rpc_request::Request::Unlink(proto::Unlink { path: path.clone() })
        }
        Request::Readdir { path } => {
            rpc_request::Request::Readdir(proto::Readdir { path: path.clone() })
        }
        Request::ReaddirFd { fd } => rpc_request::Request::ReaddirFd(proto::ReaddirFd { fd: *fd }),
        Request::Rmdir { path } => rpc_request::Request::Rmdir(proto::Rmdir { path: path.clone() }),
        Request::AppendWrite { fd, data } => {
            rpc_request::Request::AppendWrite(proto::AppendWrite {
                fd: *fd,
                data: data.clone(),
            })
        }
    };

    let proto_request = proto::RpcRequest {
        session_id: rpc_request.session_id.clone(),
        request: Some(request),
    };

    proto_request.encode_to_vec()
}

/// Convert protobuf bytes to internal Response
pub fn from_proto_response_bytes(data: &[u8]) -> Result<Response, ErrorCode> {
    let proto_response =
        proto::RpcResponse::decode(data).map_err(|_| ErrorCode::SerializationError)?;

    match proto_response.response {
        Some(rpc_response::Response::Connected(c)) => Ok(Response::Connected {
            session_id: c.session_id,
            version: c.version,
        }),
        Some(rpc_response::Response::Ok(_)) => Ok(Response::Ok),
        Some(rpc_response::Response::Fd(f)) => Ok(Response::Fd { fd: f.fd }),
        Some(rpc_response::Response::Data(d)) => Ok(Response::Data { bytes: d.bytes }),
        Some(rpc_response::Response::Written(w)) => Ok(Response::Written {
            count: w.count as usize,
        }),
        Some(rpc_response::Response::Position(p)) => Ok(Response::Position { pos: p.pos }),
        Some(rpc_response::Response::Metadata(m)) => Ok(Response::Metadata {
            metadata: Metadata {
                size: m.size,
                created: m.created,
                modified: m.modified,
                is_dir: m.is_dir,
            },
        }),
        Some(rpc_response::Response::DirEntries(d)) => Ok(Response::DirEntries {
            entries: d
                .entries
                .into_iter()
                .map(|e| DirEntry {
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect(),
        }),
        Some(rpc_response::Response::Error(e)) => Ok(Response::Error {
            code: ErrorCode::from_i32(e.code).unwrap_or(ErrorCode::Io),
            message: e.message,
        }),
        None => Err(ErrorCode::SerializationError),
    }
}
