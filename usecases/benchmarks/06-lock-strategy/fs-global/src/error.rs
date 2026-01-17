//! Error types for fs-global

/// Filesystem error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    NotEmpty,
    BadFileDescriptor,
    InvalidArgument,
    PermissionDenied,
    IoError,
}

impl core::fmt::Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FsError::NotFound => write!(f, "Not found"),
            FsError::AlreadyExists => write!(f, "Already exists"),
            FsError::NotADirectory => write!(f, "Not a directory"),
            FsError::IsADirectory => write!(f, "Is a directory"),
            FsError::NotEmpty => write!(f, "Directory not empty"),
            FsError::BadFileDescriptor => write!(f, "Bad file descriptor"),
            FsError::InvalidArgument => write!(f, "Invalid argument"),
            FsError::PermissionDenied => write!(f, "Permission denied"),
            FsError::IoError => write!(f, "I/O error"),
        }
    }
}

impl std::error::Error for FsError {}
