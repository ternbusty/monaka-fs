use fs_wasm::{FsStat, fs_close, fs_fstat, fs_mkdir, fs_open_path, fs_read, fs_seek, fs_write};
use serial_test::serial;

#[cfg(test)]
mod basic_operations {
    use super::*;

    #[test]
    #[serial]
    fn success_basic_read_write() {
        unsafe {
            // Filesystem auto-initializes on first use
            // First create the parent directories
            let _ = fs_mkdir("/tmp".as_ptr(), 4);
            let _ = fs_mkdir("/tmp/foo".as_ptr(), 8);

            let fd = fs_open_path("/tmp/foo/bar.txt".as_ptr(), 16);
            assert!(fd > 0, "fs_open_path failed with error code: {}", fd);

            let msg = b"hello wasm fs";
            let n = fs_write(fd as u32, msg.as_ptr(), msg.len() as u32);
            assert_eq!(n, msg.len() as i32);

            // Seek back to the beginning
            let seek_result = fs_seek(fd as u32, 0, 0); // SEEK_SET = 0
            assert!(seek_result >= 0);

            let mut buf = [0u8; 32];
            let m = fs_read(fd as u32, buf.as_mut_ptr(), msg.len() as u32);
            assert_eq!(m, msg.len() as i32);
            assert_eq!(&buf[..m as usize], msg);

            assert_eq!(fs_close(fd as u32), 0);
        }
    }

    #[test]
    #[serial]
    fn success_multiple_files() {
        unsafe {
            let _ = fs_mkdir("/multi".as_ptr(), 6);

            // Open and write to multiple files, one at a time
            let files = [
                ("/multi/file1.txt", b"content1"),
                ("/multi/file2.txt", b"content2"),
                ("/multi/file3.txt", b"content3"),
            ];

            // Process each file completely before moving to the next
            for (path, content) in &files {
                let fd = fs_open_path(path.as_ptr(), path.len() as u32);
                assert!(fd > 0);
                let written = fs_write(fd as u32, content.as_ptr(), content.len() as u32);
                assert_eq!(written, content.len() as i32);
                fs_close(fd as u32);
            }

            // Read back from all files
            for (path, expected) in &files {
                let fd = fs_open_path(path.as_ptr(), path.len() as u32);
                assert!(fd > 0);
                let mut buf = [0u8; 32];
                let n = fs_read(fd as u32, buf.as_mut_ptr(), expected.len() as u32);
                assert_eq!(n, expected.len() as i32);
                assert_eq!(&buf[..n as usize], *expected);
                fs_close(fd as u32);
            }
        }
    }

    #[test]
    #[serial]
    fn success_multiple_fds_independent_positions() {
        unsafe {
            let path = "/concurrent.txt";

            // First, create and write the file
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);
            fs_write(fd as u32, b"0123456789".as_ptr(), 10);
            fs_close(fd as u32);

            // Now open it twice and verify independent positions
            let fd1 = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd1 > 0);

            let fd2 = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd2 > 0);
            assert_ne!(fd1, fd2);

            // Both fds should have independent positions
            fs_seek(fd1 as u32, 0, 0);
            fs_seek(fd2 as u32, 5, 0);

            // Read from fd1 (position 0)
            let mut buf1 = [0u8; 3];
            fs_read(fd1 as u32, buf1.as_mut_ptr(), 3);
            assert_eq!(&buf1, b"012");

            // Read from fd2 (position 5)
            let mut buf2 = [0u8; 3];
            fs_read(fd2 as u32, buf2.as_mut_ptr(), 3);
            assert_eq!(&buf2, b"567");

            fs_close(fd1 as u32);
            fs_close(fd2 as u32);
        }
    }
}

#[cfg(test)]
mod seek_operations {
    use super::*;

    #[test]
    #[serial]
    fn success_all_seek_modes() {
        unsafe {
            let path = "/seek_test.txt";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Write data
            let data = b"0123456789ABCDEF";
            fs_write(fd as u32, data.as_ptr(), data.len() as u32);

            // Test SEEK_SET (whence=0)
            let pos = fs_seek(fd as u32, 5, 0);
            assert_eq!(pos, 5);
            let mut buf = [0u8; 3];
            fs_read(fd as u32, buf.as_mut_ptr(), 3);
            assert_eq!(&buf, b"567");

            // Test SEEK_CUR (whence=1)
            let pos = fs_seek(fd as u32, 2, 1); // Current pos is 8, seek +2 = 10
            assert_eq!(pos, 10);
            fs_read(fd as u32, buf.as_mut_ptr(), 3);
            assert_eq!(&buf, b"ABC");

            // Test SEEK_END (whence=2)
            let pos = fs_seek(fd as u32, -3, 2); // From end
            assert_eq!(pos, 13);
            fs_read(fd as u32, buf.as_mut_ptr(), 3);
            assert_eq!(&buf, b"DEF");

            fs_close(fd as u32);
        }
    }
}

#[cfg(test)]
mod metadata {
    use super::*;

    #[test]
    #[serial]
    fn success_fstat() {
        unsafe {
            let path = "/fstat_test.txt";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Write some data
            let data = b"test data for fstat";
            let written = fs_write(fd as u32, data.as_ptr(), data.len() as u32);
            assert_eq!(written, data.len() as i32);

            // Get file metadata
            let mut stat = FsStat {
                size: 0,
                created: 0,
                modified: 0,
            };
            let result = fs_fstat(fd as u32, &mut stat as *mut FsStat);
            assert_eq!(result, 0, "fstat should succeed");
            assert_eq!(stat.size, data.len() as u64);

            fs_close(fd as u32);
        }
    }

    #[test]
    #[serial]
    fn error_fstat_null_pointer() {
        unsafe {
            let path = "/test.txt";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Null pointer for stat output should return error
            let result = fs_fstat(fd as u32, std::ptr::null_mut());
            assert!(result < 0);

            fs_close(fd as u32);
        }
    }
}

#[cfg(test)]
mod ffi_safety {
    use super::*;

    #[test]
    #[serial]
    fn error_null_pointers() {
        unsafe {
            // Null pointer for path should return error
            let result = fs_open_path(std::ptr::null(), 10);
            assert!(result < 0, "Expected error for null path pointer");

            let result = fs_mkdir(std::ptr::null(), 10);
            assert!(result < 0, "Expected error for null mkdir path pointer");

            // Valid fd but null buffer should return error
            let path = "/null_ptr/test.txt";
            let _ = fs_mkdir("/null_ptr".as_ptr(), 9);
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            if fd > 0 {
                let result = fs_write(fd as u32, std::ptr::null(), 10);
                assert!(result < 0, "Expected error for null write buffer");

                let result = fs_read(fd as u32, std::ptr::null_mut(), 10);
                assert!(result < 0, "Expected error for null read buffer");

                fs_close(fd as u32);
            }
        }
    }

    #[test]
    #[serial]
    fn error_invalid_fd() {
        unsafe {
            // Using invalid fd should return error
            let result = fs_read(999, [0u8; 10].as_mut_ptr(), 10);
            assert!(result < 0);

            let result = fs_write(999, b"data".as_ptr(), 4);
            assert!(result < 0);

            let result = fs_seek(999, 0, 0);
            assert!(result < 0);

            let mut stat = FsStat {
                size: 0,
                created: 0,
                modified: 0,
            };
            let result = fs_fstat(999, &mut stat as *mut FsStat);
            assert!(result < 0);

            let result = fs_close(999);
            assert!(result < 0);
        }
    }

    #[test]
    #[serial]
    fn error_closed_fd() {
        unsafe {
            let path = "/test_closed.txt";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Close the file
            assert_eq!(fs_close(fd as u32), 0);

            // Using closed fd should return error
            let result = fs_read(fd as u32, [0u8; 10].as_mut_ptr(), 10);
            assert!(result < 0);

            let result = fs_write(fd as u32, b"data".as_ptr(), 4);
            assert!(result < 0);

            // Closing again should also fail
            let result = fs_close(fd as u32);
            assert!(result < 0);
        }
    }
}

#[cfg(test)]
mod edge_cases {
    use super::*;

    #[test]
    #[serial]
    fn error_empty_path() {
        unsafe {
            // Empty path should return error
            let result = fs_open_path("".as_ptr(), 0);
            assert!(result < 0, "Expected error for empty path");

            let result = fs_mkdir("".as_ptr(), 0);
            assert!(result < 0, "Expected error for empty mkdir path");
        }
    }

    #[test]
    #[serial]
    fn error_invalid_operations() {
        unsafe {
            // Invalid file descriptor should return error
            let result = fs_read(999, [0u8; 10].as_mut_ptr(), 10);
            assert!(result < 0, "Expected error for invalid fd");

            let result = fs_write(999, b"data".as_ptr(), 4);
            assert!(result < 0, "Expected error for invalid fd");

            let result = fs_close(999);
            assert!(result < 0, "Expected error for invalid fd");

            // Non-existent file without create should fail
            // Note: fs_open_path defaults to O_RDWR | O_CREAT, so we can't easily test NotFound here
            // But we can test empty path
            let result = fs_open_path("".as_ptr(), 0);
            assert!(result < 0, "Expected error for empty path");
        }
    }

    #[test]
    #[serial]
    fn edge_case_zero_length_operations() {
        unsafe {
            let path = "/zero_length.txt";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Write zero bytes should succeed but write 0
            let result = fs_write(fd as u32, b"data".as_ptr(), 0);
            assert_eq!(result, 0);

            // Read zero bytes should succeed but read 0
            let mut buf = [0u8; 10];
            let result = fs_read(fd as u32, buf.as_mut_ptr(), 0);
            assert_eq!(result, 0);

            fs_close(fd as u32);
        }
    }
}

#[cfg(test)]
mod large_files {
    use super::*;

    #[test]
    #[serial]
    fn success_large_write_read() {
        unsafe {
            let path = "/large_file.bin";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Write 16KB of data
            let large_data: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
            let written = fs_write(fd as u32, large_data.as_ptr(), large_data.len() as u32);
            assert_eq!(written, large_data.len() as i32);

            // Read it back
            fs_seek(fd as u32, 0, 0);
            let mut read_buf = vec![0u8; 16384];
            let read = fs_read(fd as u32, read_buf.as_mut_ptr(), read_buf.len() as u32);
            assert_eq!(read, large_data.len() as i32);
            assert_eq!(read_buf, large_data);

            fs_close(fd as u32);
        }
    }

    #[test]
    #[serial]
    fn success_block_boundaries() {
        unsafe {
            let path = "/boundary_test.txt";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Write exactly at block boundary (4096 bytes)
            let data = vec![0xAA; 4096];
            let written = fs_write(fd as u32, data.as_ptr(), data.len() as u32);
            assert_eq!(written, 4096);

            // Write one more byte to cross boundary
            let extra = b"X";
            let written = fs_write(fd as u32, extra.as_ptr(), 1);
            assert_eq!(written, 1);

            // Read across the boundary
            fs_seek(fd as u32, 4095, 0);
            let mut buf = [0u8; 2];
            let read = fs_read(fd as u32, buf.as_mut_ptr(), 2);
            assert_eq!(read, 2);
            assert_eq!(buf[0], 0xAA);
            assert_eq!(buf[1], b'X');

            fs_close(fd as u32);
        }
    }

    #[test]
    #[serial]
    fn success_sparse_file() {
        unsafe {
            let path = "/sparse.dat";
            let fd = fs_open_path(path.as_ptr(), path.len() as u32);
            assert!(fd > 0);

            // Write at beginning
            let start_data = b"START";
            fs_write(fd as u32, start_data.as_ptr(), start_data.len() as u32);

            // Seek far ahead
            fs_seek(fd as u32, 10000, 0);

            // Write at end
            let end_data = b"END";
            fs_write(fd as u32, end_data.as_ptr(), end_data.len() as u32);

            // Read from sparse region (should be zeros)
            fs_seek(fd as u32, 100, 0);
            let mut sparse_buf = [0xFF; 10];
            let read = fs_read(fd as u32, sparse_buf.as_mut_ptr(), 10);
            assert_eq!(read, 10);
            assert_eq!(sparse_buf, [0u8; 10]);

            // Verify start
            fs_seek(fd as u32, 0, 0);
            let mut buf = [0u8; 5];
            fs_read(fd as u32, buf.as_mut_ptr(), 5);
            assert_eq!(&buf, b"START");

            // Verify end
            fs_seek(fd as u32, 10000, 0);
            let mut buf = [0u8; 3];
            fs_read(fd as u32, buf.as_mut_ptr(), 3);
            assert_eq!(&buf, b"END");

            fs_close(fd as u32);
        }
    }
}
