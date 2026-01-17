//! Common types for S3 sync

/// Information about an S3 object
#[derive(Debug, Clone)]
pub struct S3ObjectInfo {
    /// Path in VFS (relative to sync prefix)
    pub path: String,
    /// S3 ETag (usually MD5 hash)
    pub etag: String,
    /// Last modified timestamp (Unix epoch seconds)
    pub last_modified: u64,
    /// File size in bytes
    pub size: u64,
}

/// S3 operation errors
#[derive(Debug)]
pub enum S3Error {
    Read { key: String, message: String },
    Write { key: String, message: String },
    Delete { key: String, message: String },
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3Error::Read { key, message } => write!(f, "S3 read error for {}: {}", key, message),
            S3Error::Write { key, message } => write!(f, "S3 write error for {}: {}", key, message),
            S3Error::Delete { key, message } => {
                write!(f, "S3 delete error for {}: {}", key, message)
            }
        }
    }
}

impl std::error::Error for S3Error {}
