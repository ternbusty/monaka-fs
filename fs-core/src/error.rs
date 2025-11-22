#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    NotADirectory,
    IsADirectory,
    InvalidArgument,
    BadFileDescriptor,
    PermissionDenied,
    AlreadyExists,
}

impl FsError {
    /// Convert error to errno-style error code
    pub fn to_errno(&self) -> i32 {
        match self {
            FsError::NotFound => -2,           // ENOENT
            FsError::NotADirectory => -20,     // ENOTDIR
            FsError::IsADirectory => -21,      // EISDIR
            FsError::InvalidArgument => -22,   // EINVAL
            FsError::BadFileDescriptor => -9,  // EBADF
            FsError::PermissionDenied => -13,  // EACCES
            FsError::AlreadyExists => -17,     // EEXIST
        }
    }
}
