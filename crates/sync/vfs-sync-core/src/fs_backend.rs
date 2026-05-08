//! Abstraction over the underlying VFS handle.
//!
//! The host (multi-threaded) holds `Arc<Fs>` directly thanks to fs-core's
//! `thread-safe` feature, whereas the WASI adapter wraps a non-thread-safe
//! `Fs<T>` in `Rc<RefCell<…>>`. This trait lets `SyncManager` operate on
//! whichever flavour the consumer provides without leaking those details
//! into the sync logic.
//!
//! Implementations live in the consuming crates (`vfs-sync-host`,
//! `vfs-sync-adapter`) so that `vfs-sync-core` stays free of a `fs-core`
//! dependency.

use crate::S3Error;

/// File descriptor handle, opaque to sync_manager.
pub type Fd = u32;

/// Operations the sync manager performs against the underlying filesystem.
///
/// Methods take `&self` because both implementations encapsulate their own
/// interior mutability (`Mutex` / `RefCell`). Errors are normalised to
/// `S3Error` so callers can use `?` directly when composing with S3 calls.
pub trait FsBackend {
    /// Open `path` for reading (must already exist).
    fn open_read(&self, path: &str) -> Result<Fd, S3Error>;

    /// Open `path` for writing, creating if needed and truncating to zero.
    fn open_write_truncate(&self, path: &str) -> Result<Fd, S3Error>;

    /// Read up to `buf.len()` bytes into `buf`. Returns bytes read.
    fn read(&self, fd: Fd, buf: &mut [u8]) -> Result<usize, S3Error>;

    /// Write `buf` to the open file.
    fn write(&self, fd: Fd, buf: &[u8]) -> Result<usize, S3Error>;

    /// Close the descriptor.
    fn close(&self, fd: Fd) -> Result<(), S3Error>;

    /// Return the local modification timestamp for `path`, or `0` if the
    /// file cannot be stat'd. Matches the original
    /// `stat(path).map(|m| m.modified).unwrap_or(0)` behaviour.
    fn stat_modified(&self, path: &str) -> u64;

    /// Return the size of an open file in bytes.
    fn fstat_size(&self, fd: Fd) -> Result<u64, S3Error>;

    /// Remove a file at `path`.
    fn unlink(&self, path: &str) -> Result<(), S3Error>;

    /// Recursively create directories for `path`. Best-effort: errors are
    /// swallowed by sync_manager call sites because the parent dir often
    /// already exists.
    fn mkdir_p(&self, path: &str);
}
