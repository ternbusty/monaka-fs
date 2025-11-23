#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::boxed::Box;
use core::cell::RefCell;

pub mod ffi;
mod time;

use time::WasiTimeProvider;

// Re-export FFI functions and types for external use
pub use ffi::{
    FsStat, fs_close, fs_fstat, fs_mkdir, fs_open_path, fs_open_path_with_flags, fs_read, fs_seek,
    fs_write,
};
pub use fs_core::*;

// Global filesystem state for single-threaded WASM environment
thread_local! {
    static FS: RefCell<Option<Box<fs_core::Fs<WasiTimeProvider>>>> = const { RefCell::new(None) };
}

// Helper function to get or initialize the global filesystem instance
pub(crate) fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&mut fs_core::Fs<WasiTimeProvider>) -> R,
{
    FS.with(|fs_cell| {
        let mut fs_opt = fs_cell.borrow_mut();
        if fs_opt.is_none() {
            *fs_opt = Some(Box::new(fs_core::Fs::with_time_provider(WasiTimeProvider)));
        }
        f(fs_opt.as_mut().unwrap())
    })
}
