//! Common types for S3 sync

/// Information about an S3 object, expressed as a VFS path.
#[derive(Debug, Clone)]
pub struct S3ObjectInfo {
    /// Path in VFS (relative to sync prefix), e.g. `/logs/app.log`
    pub path: String,
    /// S3 ETag (usually MD5 hash), without surrounding quotes
    pub etag: String,
    /// Last modified timestamp (Unix epoch seconds)
    pub last_modified: u64,
    /// File size in bytes
    pub size: u64,
    /// `true` when the object is a directory marker (`files/<dir>/`)
    pub is_dir: bool,
}

/// Raw metadata of an object as returned by the store, keyed by object key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMeta {
    /// Object key relative to the sync prefix (e.g. `files/logs/app.log`)
    pub key: String,
    /// ETag without surrounding quotes
    pub etag: String,
    /// Last modified timestamp (Unix epoch seconds)
    pub last_modified: u64,
    /// Object size in bytes
    pub size: u64,
}

/// Precondition attached to a write or delete.
///
/// `IfMatch` maps to the HTTP `If-Match` header (the object must exist with
/// exactly this ETag), `IfNoneMatchAny` to `If-None-Match: *` (the object
/// must not exist). S3 evaluates these atomically against the current
/// object, which is what makes them usable as a compare-and-swap primitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Precondition {
    None,
    IfMatch(String),
    IfNoneMatchAny,
}

/// S3 operation errors
#[derive(Debug, Clone)]
pub enum S3Error {
    Read {
        key: String,
        message: String,
    },
    Write {
        key: String,
        message: String,
    },
    Delete {
        key: String,
        message: String,
    },
    /// A conditional request was rejected: HTTP 412 (`PreconditionFailed`)
    /// or 409 (`ConditionalRequestConflict`). The object changed under us.
    PreconditionFailed {
        key: String,
        status: u16,
        code: String,
    },
    /// The object does not exist. For conditional writes this means a
    /// concurrent delete won; for reads it is the ordinary miss.
    NotFound {
        key: String,
    },
    /// The backend does not implement the requested conditional operation
    /// (HTTP 501). LocalStack 4.x answers `If-Match` on `DeleteObject` this
    /// way.
    NotImplemented {
        key: String,
        message: String,
    },
}

impl S3Error {
    pub fn key(&self) -> &str {
        match self {
            S3Error::Read { key, .. }
            | S3Error::Write { key, .. }
            | S3Error::Delete { key, .. }
            | S3Error::PreconditionFailed { key, .. }
            | S3Error::NotFound { key }
            | S3Error::NotImplemented { key, .. } => key,
        }
    }

    pub fn is_precondition_failed(&self) -> bool {
        matches!(self, S3Error::PreconditionFailed { .. })
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, S3Error::NotFound { .. })
    }

    pub fn is_not_implemented(&self) -> bool {
        matches!(self, S3Error::NotImplemented { .. })
    }
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3Error::Read { key, message } => write!(f, "S3 read error for {}: {}", key, message),
            S3Error::Write { key, message } => write!(f, "S3 write error for {}: {}", key, message),
            S3Error::Delete { key, message } => {
                write!(f, "S3 delete error for {}: {}", key, message)
            }
            S3Error::PreconditionFailed { key, status, code } => {
                write!(
                    f,
                    "S3 precondition failed for {} ({} {})",
                    key, status, code
                )
            }
            S3Error::NotFound { key } => write!(f, "S3 object not found: {}", key),
            S3Error::NotImplemented { key, message } => {
                write!(f, "S3 operation not implemented for {}: {}", key, message)
            }
        }
    }
}

impl std::error::Error for S3Error {}

/// Errors surfaced by the sync manager to its consumers.
///
/// Consumers map `Busy` to WASI `error-code::busy` and `Conflict` to
/// `error-code::not-recoverable`; everything else is an I/O error.
#[derive(Debug, Clone)]
pub enum SyncError {
    /// The per-file lease could not be acquired within the configured
    /// timeout because another instance holds it.
    Busy {
        path: String,
        holder: Option<String>,
    },
    /// A conditional write or delete lost against a concurrent change.
    /// When `refreshed` is `true` the local copy has been replaced with the
    /// current S3 content so the application can re-read and retry.
    Conflict {
        path: String,
        refreshed: bool,
    },
    /// Local filesystem failure while syncing `path`.
    Fs {
        path: String,
        message: String,
    },
    S3(S3Error),
}

impl SyncError {
    pub fn is_busy(&self) -> bool {
        matches!(self, SyncError::Busy { .. })
    }

    pub fn is_conflict(&self) -> bool {
        matches!(self, SyncError::Conflict { .. })
    }
}

impl From<S3Error> for SyncError {
    fn from(e: S3Error) -> Self {
        SyncError::S3(e)
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Busy { path, holder } => match holder {
                Some(h) => write!(f, "{} is locked by instance {}", path, h),
                None => write!(f, "{} is locked by another instance", path),
            },
            SyncError::Conflict { path, refreshed } => {
                if *refreshed {
                    write!(
                        f,
                        "{} changed in S3 concurrently (local copy refreshed)",
                        path
                    )
                } else {
                    write!(f, "{} changed in S3 concurrently", path)
                }
            }
            SyncError::Fs { path, message } => {
                write!(f, "filesystem error for {}: {}", path, message)
            }
            SyncError::S3(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for SyncError {}
