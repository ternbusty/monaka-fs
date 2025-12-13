//! RPC message definitions for VFS operations

use serde::{Deserialize, Serialize};

/// Wrapper for RPC requests with session tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Session ID assigned by server on Connect (None for Connect request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<u64>,
    /// The actual request
    pub request: Request,
}

/// RPC request messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Connect to VFS server and establish session
    Connect { version: u32 },

    /// Open file at path with flags
    OpenPath { path: String, flags: u32 },

    /// Open file relative to directory file descriptor
    OpenAt {
        dir_fd: u32,
        path: String,
        flags: u32,
    },

    /// Read from file descriptor
    Read { fd: u32, length: usize },

    /// Write to file descriptor
    Write { fd: u32, data: Vec<u8> },

    /// Close file descriptor
    Close { fd: u32 },

    /// Seek in file
    Seek { fd: u32, offset: i64, whence: i32 },

    /// Truncate file to specified size
    Ftruncate { fd: u32, size: u64 },

    /// Get file metadata by path
    Stat { path: String },

    /// Get file metadata by file descriptor
    Fstat { fd: u32 },

    /// Create directory
    Mkdir { path: String },

    /// Create directory and all parent directories
    MkdirP { path: String },

    /// Remove file
    Unlink { path: String },

    /// Read directory entries by path
    Readdir { path: String },

    /// Read directory entries by file descriptor
    ReaddirFd { fd: u32 },

    /// Remove empty directory
    Rmdir { path: String },
}

/// RPC response messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Connection established
    Connected { session_id: u64, version: u32 },

    /// Operation succeeded without data
    Ok,

    /// File descriptor allocated
    Fd { fd: u32 },

    /// Data read from file
    Data { bytes: Vec<u8> },

    /// Number of bytes written
    Written { count: usize },

    /// File position after seek
    Position { pos: u64 },

    /// File or directory metadata
    Metadata { metadata: Metadata },

    /// Directory entries
    DirEntries { entries: Vec<DirEntry> },

    /// Error occurred
    Error { code: ErrorCode, message: String },
}

/// File metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub size: u64,
    pub created: u64,
    pub modified: u64,
    pub is_dir: bool,
}

/// Directory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Error codes matching fs-core FsError
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
}

impl core::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = Request::OpenPath {
            path: "/test.txt".to_string(),
            flags: 0x42,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: Request = serde_json::from_str(&json).unwrap();

        match deserialized {
            Request::OpenPath { path, flags } => {
                assert_eq!(path, "/test.txt");
                assert_eq!(flags, 0x42);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_response_serialization() {
        let response = Response::Fd { fd: 42 };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: Response = serde_json::from_str(&json).unwrap();

        match deserialized {
            Response::Fd { fd } => assert_eq!(fd, 42),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_error_response() {
        let response = Response::Error {
            code: ErrorCode::NotFound,
            message: "File not found".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: Response = serde_json::from_str(&json).unwrap();

        match deserialized {
            Response::Error { code, message } => {
                assert_eq!(code, ErrorCode::NotFound);
                assert_eq!(message, "File not found");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_metadata_serialization() {
        let metadata = Metadata {
            size: 1024,
            created: 1000,
            modified: 2000,
            is_dir: false,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: Metadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.size, 1024);
        assert_eq!(deserialized.created, 1000);
        assert_eq!(deserialized.modified, 2000);
        assert!(!deserialized.is_dir);
    }
}
