use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotADirectory,
    IsADirectory,
    InvalidArgument,
    BadFileDescriptor,
    PermissionDenied,
    AlreadyExists,
    NotEmpty,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NotFound => write!(f, "Not found"),
            FsError::NotADirectory => write!(f, "Not a directory"),
            FsError::IsADirectory => write!(f, "Is a directory"),
            FsError::InvalidArgument => write!(f, "Invalid argument"),
            FsError::BadFileDescriptor => write!(f, "Bad file descriptor"),
            FsError::PermissionDenied => write!(f, "Permission denied"),
            FsError::AlreadyExists => write!(f, "Already exists"),
            FsError::NotEmpty => write!(f, "Directory not empty"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for FsError {}

impl FsError {
    /// Convert error to errno-style error code
    pub fn to_errno(&self) -> i32 {
        match self {
            FsError::NotFound => -2,          // ENOENT
            FsError::NotADirectory => -20,    // ENOTDIR
            FsError::IsADirectory => -21,     // EISDIR
            FsError::InvalidArgument => -22,  // EINVAL
            FsError::BadFileDescriptor => -9, // EBADF
            FsError::PermissionDenied => -13, // EACCES
            FsError::AlreadyExists => -17,    // EEXIST
            FsError::NotEmpty => -39,         // ENOTEMPTY
        }
    }

    /// Convert error to WASI error-code (u8)
    /// Reference: WASI Preview 2 filesystem error-code enum
    /// https://github.com/WebAssembly/wasi-filesystem/blob/main/wit/types.wit
    pub fn to_wasi_error_code(&self) -> u8 {
        match self {
            FsError::NotFound => 44,         // noent
            FsError::NotADirectory => 54,    // notdir
            FsError::IsADirectory => 31,     // isdir
            FsError::InvalidArgument => 28,  // inval
            FsError::BadFileDescriptor => 8, // badf
            FsError::PermissionDenied => 2,  // access
            FsError::AlreadyExists => 20,    // exist
            FsError::NotEmpty => 55,         // notempty
        }
    }
}
