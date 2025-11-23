use fs_core::{Fd, FsError};

use crate::with_fs;

/// File metadata structure for FFI
#[repr(C)]
pub struct FsStat {
    pub size: u64,
    pub created: u64,
    pub modified: u64,
}

/// Opens a file at the given path.
///
/// # Safety
///
/// The caller must ensure that `ptr` is valid and points to `len` bytes of valid UTF-8 data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fs_open_path(ptr: *const u8, len: u32) -> i32 {
    if ptr.is_null() {
        return FsError::InvalidArgument.to_errno();
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
    let path = match core::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return FsError::InvalidArgument.to_errno(),
    };
    with_fs(|fs| {
        fs.open_path(path)
            .map(|fd| fd as i32)
            .unwrap_or_else(|e| e.to_errno())
    })
}

/// Opens a file at the given path with specific flags.
///
/// # Safety
///
/// The caller must ensure that `ptr` is valid and points to `len` bytes of valid UTF-8 data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fs_open_path_with_flags(ptr: *const u8, len: u32, flags: u32) -> i32 {
    if ptr.is_null() {
        return FsError::InvalidArgument.to_errno();
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
    let path = match core::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return FsError::InvalidArgument.to_errno(),
    };
    with_fs(|fs| {
        fs.open_path_with_flags(path, flags)
            .map(|fd| fd as i32)
            .unwrap_or_else(|e| e.to_errno())
    })
}

/// Writes data to a file descriptor.
///
/// # Safety
///
/// The caller must ensure that `ptr` is valid and points to `len` bytes of readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fs_write(fd: Fd, ptr: *const u8, len: u32) -> i32 {
    if ptr.is_null() {
        return FsError::InvalidArgument.to_errno();
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
    with_fs(|fs| {
        fs.write(fd, slice)
            .map(|n| n as i32)
            .unwrap_or_else(|e| e.to_errno())
    })
}

/// Reads data from a file descriptor.
///
/// # Safety
///
/// The caller must ensure that `ptr` is valid and points to `len` bytes of writable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fs_read(fd: Fd, ptr: *mut u8, len: u32) -> i32 {
    if ptr.is_null() {
        return FsError::InvalidArgument.to_errno();
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr, len as usize) };
    with_fs(|fs| {
        fs.read(fd, slice)
            .map(|n| n as i32)
            .unwrap_or_else(|e| e.to_errno())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn fs_close(fd: Fd) -> i32 {
    with_fs(|fs| fs.close(fd).map(|_| 0).unwrap_or_else(|e| e.to_errno()))
}

#[unsafe(no_mangle)]
pub extern "C" fn fs_seek(fd: Fd, offset: i64, whence: i32) -> i64 {
    with_fs(|fs| {
        fs.seek(fd, offset, whence)
            .map(|pos| pos as i64)
            .unwrap_or_else(|e| e.to_errno() as i64)
    })
}

/// Retrieves file metadata for a file descriptor.
///
/// # Safety
///
/// The caller must ensure that `stat_out` is valid and points to writable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fs_fstat(fd: Fd, stat_out: *mut FsStat) -> i32 {
    if stat_out.is_null() {
        return FsError::InvalidArgument.to_errno();
    }
    with_fs(|fs| match fs.fstat(fd) {
        Ok(metadata) => {
            unsafe {
                (*stat_out).size = metadata.size;
                (*stat_out).created = metadata.created;
                (*stat_out).modified = metadata.modified;
            }
            0
        }
        Err(e) => e.to_errno(),
    })
}

/// Creates a directory at the given path.
///
/// # Safety
///
/// The caller must ensure that `ptr` is valid and points to `len` bytes of valid UTF-8 data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fs_mkdir(ptr: *const u8, len: u32) -> i32 {
    if ptr.is_null() {
        return FsError::InvalidArgument.to_errno();
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
    let path = match core::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return FsError::InvalidArgument.to_errno(),
    };
    with_fs(|fs| fs.mkdir(path).map(|_| 0).unwrap_or_else(|e| e.to_errno()))
}
