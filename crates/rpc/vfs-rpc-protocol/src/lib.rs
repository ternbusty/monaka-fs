//! VFS RPC Protocol
//!
//! Defines message types and serialization for VFS RPC communication
//! between client and server over TCP sockets.

pub mod messages;

pub use messages::{DirEntry, ErrorCode, Metadata, Request, Response, RpcRequest};

/// Protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Default server port
pub const DEFAULT_PORT: u16 = 9000;
