//! VFS RPC Protocol
//!
//! Defines message types and serialization for VFS RPC communication
//! between client and server over TCP sockets.
//!
//! Uses Protocol Buffers for efficient binary serialization.

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
            _ => None,
        }
    }
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
