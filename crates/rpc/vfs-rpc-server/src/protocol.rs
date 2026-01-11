//! Internal protocol types and conversion to/from protobuf

use vfs_rpc_protocol::{self as proto, rpc_request, rpc_response, ErrorCode, PROTOCOL_VERSION};

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

/// RPC request with session tracking
#[derive(Debug, Clone)]
pub struct RpcRequest {
    pub session_id: Option<String>,
    pub request: Request,
}

/// Convert protobuf RpcRequest to internal RpcRequest
pub fn from_proto_request(proto_req: proto::RpcRequest) -> Result<RpcRequest, &'static str> {
    let session_id = proto_req.session_id;
    let request = match proto_req.request {
        Some(rpc_request::Request::Connect(c)) => Request::Connect { version: c.version },
        Some(rpc_request::Request::OpenPath(o)) => Request::OpenPath {
            path: o.path,
            flags: o.flags,
        },
        Some(rpc_request::Request::OpenAt(o)) => Request::OpenAt {
            dir_fd: o.dir_fd,
            path: o.path,
            flags: o.flags,
        },
        Some(rpc_request::Request::Read(r)) => Request::Read {
            fd: r.fd,
            length: r.length as usize,
        },
        Some(rpc_request::Request::Write(w)) => Request::Write {
            fd: w.fd,
            data: w.data,
        },
        Some(rpc_request::Request::Close(c)) => Request::Close { fd: c.fd },
        Some(rpc_request::Request::Seek(s)) => Request::Seek {
            fd: s.fd,
            offset: s.offset,
            whence: s.whence,
        },
        Some(rpc_request::Request::Ftruncate(f)) => Request::Ftruncate {
            fd: f.fd,
            size: f.size,
        },
        Some(rpc_request::Request::Stat(s)) => Request::Stat { path: s.path },
        Some(rpc_request::Request::Fstat(f)) => Request::Fstat { fd: f.fd },
        Some(rpc_request::Request::Mkdir(m)) => Request::Mkdir { path: m.path },
        Some(rpc_request::Request::MkdirP(m)) => Request::MkdirP { path: m.path },
        Some(rpc_request::Request::Unlink(u)) => Request::Unlink { path: u.path },
        Some(rpc_request::Request::Readdir(r)) => Request::Readdir { path: r.path },
        Some(rpc_request::Request::ReaddirFd(r)) => Request::ReaddirFd { fd: r.fd },
        Some(rpc_request::Request::Rmdir(r)) => Request::Rmdir { path: r.path },
        Some(rpc_request::Request::AppendWrite(a)) => Request::AppendWrite {
            fd: a.fd,
            data: a.data,
        },
        None => return Err("Missing request"),
    };
    Ok(RpcRequest {
        session_id,
        request,
    })
}

/// Convert internal Response to protobuf RpcResponse
pub fn to_proto_response(response: Response) -> proto::RpcResponse {
    let response = match response {
        Response::Connected {
            session_id,
            version,
        } => rpc_response::Response::Connected(proto::Connected {
            session_id,
            version,
        }),
        Response::Ok => rpc_response::Response::Ok(proto::Ok {}),
        Response::Fd { fd } => rpc_response::Response::Fd(proto::Fd { fd }),
        Response::Data { bytes } => rpc_response::Response::Data(proto::Data { bytes }),
        Response::Written { count } => rpc_response::Response::Written(proto::Written {
            count: count as u64,
        }),
        Response::Position { pos } => rpc_response::Response::Position(proto::Position { pos }),
        Response::Metadata { metadata } => {
            rpc_response::Response::Metadata(proto::MetadataResponse {
                size: metadata.size,
                created: metadata.created,
                modified: metadata.modified,
                is_dir: metadata.is_dir,
            })
        }
        Response::DirEntries { entries } => rpc_response::Response::DirEntries(proto::DirEntries {
            entries: entries
                .into_iter()
                .map(|e| proto::DirEntry {
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect(),
        }),
        Response::Error { code, message } => rpc_response::Response::Error(proto::Error {
            code: code as i32,
            message,
        }),
    };
    proto::RpcResponse {
        response: Some(response),
    }
}
