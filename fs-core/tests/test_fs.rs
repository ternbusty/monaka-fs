use fs_core::{Fs, FsError, O_APPEND, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};

#[cfg(test)]
mod open {
    use super::*;

    #[test]
    fn success_basic_open_write_read() {
        // given
        let mut fs = Fs::new();
        // Create parent directories explicitly
        fs.mkdir_p("/tmp/foo").unwrap();
        let fd = fs.open_path("/tmp/foo/bar.txt").unwrap();
        // when
        fs.write(fd, b"abc").unwrap();
        // seek back to beginning
        fs.seek(fd, 0, 0).unwrap(); // SEEK_SET = 0
        // then
        let mut buf = [0u8; 8];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"abc");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_open_existing_file() {
        let mut fs = Fs::new();
        // Create a file
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"Original").unwrap();
        fs.close(fd).unwrap();
        // Open existing file without O_CREAT should work
        let fd = fs.open_path_with_flags("/test.txt", O_RDONLY).unwrap();
        // Should be able to read original content
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Original");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_with_rdonly_flag() {
        let mut fs = Fs::new();
        // Create a file first
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"Hello").unwrap();
        fs.close(fd).unwrap();
        // Open with O_RDONLY
        let fd = fs.open_path_with_flags("/test.txt", O_RDONLY).unwrap();
        // Should be able to read
        let mut buf = [0u8; 10];
        fs.seek(fd, 0, 0).unwrap();
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello");
        // Should NOT be able to write
        let result = fs.write(fd, b"World");
        assert!(matches!(result, Err(FsError::PermissionDenied)));
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_with_wronly_flag() {
        let mut fs = Fs::new();
        // Open with O_WRONLY
        let fd = fs
            .open_path_with_flags("/test.txt", O_WRONLY | O_CREAT)
            .unwrap();
        // Should be able to write
        let result = fs.write(fd, b"Data");
        assert!(result.is_ok());
        // Seek to beginning
        fs.seek(fd, 0, 0).unwrap();
        // Should NOT be able to read
        let mut buf = [0u8; 10];
        let result = fs.read(fd, &mut buf);
        assert!(matches!(result, Err(FsError::PermissionDenied)));
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_with_rdwr_flag() {
        let mut fs = Fs::new();
        // Open with O_RDWR
        let fd = fs
            .open_path_with_flags("/test.txt", O_RDWR | O_CREAT)
            .unwrap();
        // Should be able to write
        fs.write(fd, b"ReadWrite").unwrap();
        // Should be able to read
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"ReadWrite");
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_opening_directory() {
        let mut fs = Fs::new();
        // Create a directory
        fs.mkdir("/testdir").unwrap();
        // Trying to open a directory should fail with IsADirectory error
        let result = fs.open_path("/testdir");
        assert!(matches!(result, Err(FsError::IsADirectory)));
    }

    #[test]
    fn error_without_create_flag() {
        let mut fs = Fs::new();
        // Opening non-existent file without O_CREAT should fail
        let result = fs.open_path_with_flags("/nonexistent.txt", O_RDWR);
        assert!(matches!(result, Err(FsError::NotFound)));
        // With O_CREAT it should succeed
        let fd = fs
            .open_path_with_flags("/nonexistent.txt", O_RDWR | O_CREAT)
            .unwrap();
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_parent_not_exists() {
        let mut fs = Fs::new();
        // open should fail if parent directory doesn't exist
        let result = fs.open_path("/nonexistent/file.txt");
        assert!(matches!(result, Err(FsError::NotFound)));
        // After creating parent, open should succeed
        fs.mkdir("/nonexistent").unwrap();
        let fd = fs.open_path("/nonexistent/file.txt").unwrap();
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_using_closed_fd() {
        let mut fs = Fs::new();
        // Open and close a file
        let fd = fs.open_path("/test.txt").unwrap();
        fs.close(fd).unwrap();
        // Using closed fd should return BadFileDescriptor
        let result = fs.read(fd, &mut [0u8; 10]);
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
        let result = fs.write(fd, b"data");
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
    }
}

#[cfg(test)]
mod read_write {
    use super::*;

    #[test]
    fn success_multiple_fds_independent_positions() {
        let mut fs = Fs::new();
        // Create and write to a file
        let fd1 = fs.open_path("/shared.txt").unwrap();
        fs.write(fd1, b"0123456789").unwrap();
        // Open the same file again
        let fd2 = fs.open_path("/shared.txt").unwrap();
        // Each fd should have independent position
        fs.seek(fd1, 0, 0).unwrap();
        fs.seek(fd2, 5, 0).unwrap();
        // Read from fd1
        let mut buf1 = [0u8; 3];
        fs.read(fd1, &mut buf1).unwrap();
        assert_eq!(&buf1, b"012");
        // Read from fd2 (should start at position 5)
        let mut buf2 = [0u8; 3];
        fs.read(fd2, &mut buf2).unwrap();
        assert_eq!(&buf2, b"567");
        // Positions should be independent
        // fd1 should be at position 3
        let mut buf3 = [0u8; 2];
        fs.read(fd1, &mut buf3).unwrap();
        assert_eq!(&buf3, b"34");
        // fd2 should be at position 8
        let mut buf4 = [0u8; 2];
        fs.read(fd2, &mut buf4).unwrap();
        assert_eq!(&buf4, b"89");
        fs.close(fd1).unwrap();
        fs.close(fd2).unwrap();
    }

    #[test]
    fn success_write_at_specific_position() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        // Write initial data
        fs.write(fd, b"0123456789").unwrap();
        // Seek to position 3
        fs.seek(fd, 3, 0).unwrap();
        // Overwrite with "ABC"
        fs.write(fd, b"ABC").unwrap();
        // Read back the entire file
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"012ABC6789");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_write_beyond_eof() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        // Write initial data
        fs.write(fd, b"Hello").unwrap();
        // Seek way beyond current EOF
        fs.seek(fd, 100, 0).unwrap();
        // Write at position 100
        fs.write(fd, b"World").unwrap();
        // File size should now include the sparse region
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, 105); // 100 + 5
        // Read from the sparse region (should be zeros)
        fs.seek(fd, 10, 0).unwrap();
        let mut buf = [0u8; 10];
        fs.read(fd, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 10]);
        // Read from position 100
        fs.seek(fd, 100, 0).unwrap();
        let mut buf2 = [0u8; 5];
        fs.read(fd, &mut buf2).unwrap();
        assert_eq!(&buf2, b"World");
        fs.close(fd).unwrap();
    }

    #[test]
    fn edge_case_empty_file() {
        let mut fs = Fs::new();
        // Create an empty file
        let fd = fs.open_path("/empty.txt").unwrap();
        // Reading from empty file should return 0
        let mut buf = [0u8; 10];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 0);
        // File size should be 0
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, 0);
        fs.close(fd).unwrap();
    }

    #[test]
    fn edge_case_read_past_eof() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        // Write 5 bytes
        fs.write(fd, b"Hello").unwrap();
        // Try to read 100 bytes - should only get 5
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 100];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..n], b"Hello");
        // Reading again should return 0 (at EOF)
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 0);
        fs.close(fd).unwrap();
    }

    #[test]
    fn edge_case_read_after_write_without_seek() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        // Write data
        fs.write(fd, b"Hello").unwrap();
        // Position is now at end of file (5)
        // Reading without seeking should return 0
        let mut buf = [0u8; 10];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 0);
        // After seeking to beginning, we can read
        fs.seek(fd, 0, 0).unwrap();
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello");
        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod seek {
    use super::*;

    #[test]
    fn success_seek_cur() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/seek_test.txt").unwrap();
        // Write some data
        fs.write(fd, b"0123456789").unwrap();
        // Seek to beginning
        fs.seek(fd, 0, 0).unwrap(); // SEEK_SET
        // Read first 5 bytes
        let mut buf = [0u8; 5];
        fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf, b"01234");
        // Now position is at 5. Seek forward 2 bytes from current position
        let pos = fs.seek(fd, 2, 1).unwrap(); // SEEK_CUR
        assert_eq!(pos, 7);
        // Read from position 7
        let mut buf2 = [0u8; 3];
        let n = fs.read(fd, &mut buf2).unwrap();
        assert_eq!(&buf2[..n], b"789");
        // Seek backward from current position
        fs.seek(fd, -5, 1).unwrap(); // SEEK_CUR backward
        let mut buf3 = [0u8; 2];
        fs.read(fd, &mut buf3).unwrap();
        assert_eq!(&buf3, b"56");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_seek_end() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/seek_end.txt").unwrap();
        // Write data
        fs.write(fd, b"Hello World").unwrap();
        // Seek to 5 bytes before the end
        let pos = fs.seek(fd, -5, 2).unwrap(); // SEEK_END
        assert_eq!(pos, 6);
        // Read from that position
        let mut buf = [0u8; 5];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"World");
        // Seek to the end
        let pos = fs.seek(fd, 0, 2).unwrap(); // SEEK_END
        assert_eq!(pos, 11);
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_invalid_whence() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        // Invalid whence value should return error
        let result = fs.seek(fd, 0, 99);
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_negative_offsets() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        // Write some data
        fs.write(fd, b"0123456789").unwrap();
        // SEEK_SET with negative offset should fail
        let result = fs.seek(fd, -5, 0); // SEEK_SET
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        // SEEK_CUR with negative offset that goes below 0 should fail
        fs.seek(fd, 3, 0).unwrap(); // Position at 3
        let result = fs.seek(fd, -5, 1); // SEEK_CUR, would be -2
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        // SEEK_END with large negative offset that goes below 0 should fail
        let result = fs.seek(fd, -20, 2); // SEEK_END, file size is 10, would be -10
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        // Valid negative SEEK_CUR should work
        fs.seek(fd, 5, 0).unwrap(); // Position at 5
        let pos = fs.seek(fd, -2, 1).unwrap(); // SEEK_CUR, should be 3
        assert_eq!(pos, 3);
        // Valid negative SEEK_END should work
        let pos = fs.seek(fd, -3, 2).unwrap(); // SEEK_END, file size is 10, should be 7
        assert_eq!(pos, 7);
        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod stat_fstat {
    use super::*;

    #[test]
    fn success_stat_and_fstat() {
        let mut fs = Fs::new();
        // Create directory and file
        fs.mkdir("/data").unwrap();
        let fd = fs.open_path("/data/file.txt").unwrap();
        // Write data
        let data = b"test data for stat";
        fs.write(fd, data).unwrap();
        // Test fstat (via file descriptor)
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, data.len() as u64);
        // Test stat (via path)
        let metadata2 = fs.stat("/data/file.txt").unwrap();
        assert_eq!(metadata2.size, data.len() as u64);
        assert_eq!(metadata2.permissions, 0o644);
        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod mkdir {
    use super::*;

    #[test]
    fn success_mkdir_and_mkdir_p() {
        let mut fs = Fs::new();
        // mkdir should succeed
        assert!(fs.mkdir("/test").is_ok());
        // mkdir should fail if already exists
        assert!(matches!(fs.mkdir("/test"), Err(FsError::AlreadyExists)));
        // mkdir should fail if parent doesn't exist
        assert!(matches!(fs.mkdir("/foo/bar"), Err(FsError::NotFound)));
        // mkdir_p should create all parents
        assert!(fs.mkdir_p("/foo/bar/baz").is_ok());
        // Verify directories were created
        assert!(fs.stat("/foo").is_ok());
        assert!(fs.stat("/foo/bar").is_ok());
        assert!(fs.stat("/foo/bar/baz").is_ok());
    }

    #[test]
    fn success_parent_mtime_updates() {
        let mut fs = Fs::new();
        // Create parent directory
        fs.mkdir("/parent").unwrap();
        let initial_metadata = fs.stat("/parent").unwrap();
        let initial_mtime = initial_metadata.modified;
        // Create a file in the directory - parent mtime should update
        let fd = fs.open_path("/parent/file.txt").unwrap();
        fs.close(fd).unwrap();
        let after_file_metadata = fs.stat("/parent").unwrap();
        assert!(
            after_file_metadata.modified > initial_mtime,
            "Parent directory mtime should update when file is created"
        );
        // Create a subdirectory - parent mtime should update again
        let mtime_before_subdir = after_file_metadata.modified;
        fs.mkdir("/parent/subdir").unwrap();
        let after_subdir_metadata = fs.stat("/parent").unwrap();
        assert!(
            after_subdir_metadata.modified > mtime_before_subdir,
            "Parent directory mtime should update when subdirectory is created"
        );
    }

    #[test]
    fn success_root_has_timestamps() {
        let fs = Fs::new();
        // Get root directory metadata
        let root_metadata = fs.stat("/").unwrap();
        // Root directory should have timestamp 0 (first timestamp from MonotonicCounter)
        assert_eq!(
            root_metadata.created, 0,
            "Root directory should have created timestamp 0"
        );
        assert_eq!(
            root_metadata.modified, 0,
            "Root directory should have modified timestamp 0"
        );
        assert_eq!(
            root_metadata.created, root_metadata.modified,
            "Root directory created and modified should be equal initially"
        );
    }
}

#[cfg(test)]
mod unlink {
    use super::*;

    #[test]
    fn success_delete_file() {
        let mut fs = Fs::new();
        // Create a file
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"content").unwrap();
        fs.close(fd).unwrap();
        // Verify file exists
        assert!(fs.stat("/test.txt").is_ok());
        // Unlink the file
        fs.unlink("/test.txt").unwrap();
        // Verify file is gone
        assert!(matches!(fs.stat("/test.txt"), Err(FsError::NotFound)));
    }

    #[test]
    fn success_parent_mtime_updates() {
        let mut fs = Fs::new();
        // Create directory and file
        fs.mkdir("/dir").unwrap();
        let fd = fs.open_path("/dir/file.txt").unwrap();
        fs.close(fd).unwrap();
        // Get initial parent mtime
        let metadata_before = fs.stat("/dir").unwrap();
        // Unlink the file
        fs.unlink("/dir/file.txt").unwrap();
        // Parent directory mtime should be updated
        let metadata_after = fs.stat("/dir").unwrap();
        assert!(
            metadata_after.modified > metadata_before.modified,
            "Parent directory mtime should update when file is deleted"
        );
    }

    #[test]
    fn error_various_cases() {
        let mut fs = Fs::new();
        // Unlink non-existent file should fail
        let result = fs.unlink("/nonexistent.txt");
        assert!(matches!(result, Err(FsError::NotFound)));
        // Unlink directory should fail with IsADirectory
        fs.mkdir("/testdir").unwrap();
        let result = fs.unlink("/testdir");
        assert!(matches!(result, Err(FsError::IsADirectory)));
        // Unlink empty path should fail
        let result = fs.unlink("");
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        // Unlink root should fail
        let result = fs.unlink("/");
        assert!(matches!(result, Err(FsError::InvalidArgument)));
    }
}

#[cfg(test)]
mod readdir {
    use super::*;

    #[test]
    fn success_empty_directory() {
        let fs = Fs::new();
        // Root directory should be empty initially
        let entries = fs.readdir("/").unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn success_with_files() {
        let mut fs = Fs::new();
        // Create some files and directories
        fs.mkdir("/dir1").unwrap();
        fs.mkdir("/dir2").unwrap();
        let fd1 = fs.open_path("/file1.txt").unwrap();
        fs.close(fd1).unwrap();
        let fd2 = fs.open_path("/file2.txt").unwrap();
        fs.close(fd2).unwrap();
        // Read root directory
        let mut entries = fs.readdir("/").unwrap();
        entries.sort();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries, vec!["dir1", "dir2", "file1.txt", "file2.txt"]);
    }

    #[test]
    fn success_subdirectory() {
        let mut fs = Fs::new();
        // Create subdirectory with files
        fs.mkdir("/subdir").unwrap();
        let fd1 = fs.open_path("/subdir/a.txt").unwrap();
        fs.close(fd1).unwrap();
        let fd2 = fs.open_path("/subdir/b.txt").unwrap();
        fs.close(fd2).unwrap();
        // Read subdirectory
        let mut entries = fs.readdir("/subdir").unwrap();
        entries.sort();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries, vec!["a.txt", "b.txt"]);
        // Root should not contain these files
        let root_entries = fs.readdir("/").unwrap();
        assert_eq!(root_entries.len(), 1); // Only "subdir"
    }

    #[test]
    fn error_various_cases() {
        let mut fs = Fs::new();
        // readdir on non-existent directory
        let result = fs.readdir("/nonexistent");
        assert!(matches!(result, Err(FsError::NotFound)));
        // readdir on file should fail
        let fd = fs.open_path("/file.txt").unwrap();
        fs.close(fd).unwrap();
        let result = fs.readdir("/file.txt");
        assert!(matches!(result, Err(FsError::NotADirectory)));
        // readdir with empty path
        let result = fs.readdir("");
        assert!(matches!(result, Err(FsError::InvalidArgument)));
    }
}

#[cfg(test)]
mod ftruncate {
    use super::*;

    #[test]
    fn success_shrink_file() {
        let mut fs = Fs::new();
        // Create file with content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"0123456789").unwrap();
        // Verify initial size
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, 10);
        // Truncate to smaller size
        fs.ftruncate(fd, 5).unwrap();
        // Verify new size
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, 5);
        // Read back data
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 10];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"01234");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_expand_file() {
        let mut fs = Fs::new();
        // Create file with content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"12345").unwrap();
        // Truncate to larger size
        fs.ftruncate(fd, 10).unwrap();
        // Verify new size
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, 10);
        // Read back data - expanded region should be zeros
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0xFFu8; 10];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 10);
        assert_eq!(&buf[..5], b"12345");
        assert_eq!(&buf[5..10], &[0, 0, 0, 0, 0]);
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_truncate_to_zero() {
        let mut fs = Fs::new();
        // Create file with content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"some content").unwrap();
        // Truncate to zero
        fs.ftruncate(fd, 0).unwrap();
        // Verify size is zero
        let metadata = fs.fstat(fd).unwrap();
        assert_eq!(metadata.size, 0);
        // Read should return 0 bytes
        let mut buf = [0u8; 10];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 0);
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_updates_mtime() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"content").unwrap();
        let metadata_before = fs.fstat(fd).unwrap();
        // Truncate
        fs.ftruncate(fd, 3).unwrap();
        let metadata_after = fs.fstat(fd).unwrap();
        assert!(
            metadata_after.modified > metadata_before.modified,
            "ftruncate should update modified time"
        );
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_invalid_fd_and_permissions() {
        let mut fs = Fs::new();
        // Truncate invalid fd
        let result = fs.ftruncate(999, 100);
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
        // Truncate readonly file
        let fd_create = fs.open_path("/test.txt").unwrap();
        fs.write(fd_create, b"data").unwrap();
        fs.close(fd_create).unwrap();
        let fd = fs.open_path_with_flags("/test.txt", O_RDONLY).unwrap();
        let result = fs.ftruncate(fd, 5);
        assert!(matches!(result, Err(FsError::PermissionDenied)));
        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod timestamps {
    use super::*;

    #[test]
    fn success_file_creation() {
        let mut fs = Fs::new();
        // Create a file
        let fd = fs.open_path("/test.txt").unwrap();
        // Get metadata
        let metadata = fs.fstat(fd).unwrap();
        // created and modified timestamps should be set
        // Root directory gets timestamp 0, so first file will have timestamp 1
        assert_eq!(metadata.created, 1);
        assert_eq!(metadata.modified, 1);
        // For a newly created file, created and modified should be equal
        assert_eq!(metadata.created, metadata.modified);
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_write_updates_mtime() {
        let mut fs = Fs::new();
        // Create a file and get initial metadata
        let fd = fs.open_path("/test.txt").unwrap();
        let metadata1 = fs.fstat(fd).unwrap();
        let initial_created = metadata1.created;
        let initial_modified = metadata1.modified;
        // Write to the file
        fs.write(fd, b"data").unwrap();
        // Get metadata again
        let metadata2 = fs.fstat(fd).unwrap();
        // created should remain the same
        assert_eq!(metadata2.created, initial_created);
        // modified should have been updated (incremented by MonotonicCounter)
        assert!(metadata2.modified > initial_modified);
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_mkdir_timestamps() {
        let mut fs = Fs::new();
        // Create a directory
        fs.mkdir("/testdir").unwrap();
        // Get directory metadata
        let metadata = fs.stat("/testdir").unwrap();
        // Timestamps should be set
        // Root directory gets timestamp 0, so first created directory will have timestamp 1
        assert_eq!(metadata.created, 1);
        assert_eq!(metadata.modified, 1);
        // For a newly created directory, created and modified should be equal
        assert_eq!(metadata.created, metadata.modified);
    }

    #[test]
    fn success_multiple_writes() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        let metadata1 = fs.fstat(fd).unwrap();
        // First write
        fs.write(fd, b"first").unwrap();
        let metadata2 = fs.fstat(fd).unwrap();
        assert!(metadata2.modified > metadata1.modified);
        // Second write
        fs.write(fd, b"second").unwrap();
        let metadata3 = fs.fstat(fd).unwrap();
        assert!(metadata3.modified > metadata2.modified);
        // created should remain unchanged
        assert_eq!(metadata3.created, metadata1.created);
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_read_does_not_update() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"data").unwrap();
        let metadata1 = fs.fstat(fd).unwrap();
        // Read from file
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 10];
        fs.read(fd, &mut buf).unwrap();
        let metadata2 = fs.fstat(fd).unwrap();
        // modified timestamp should NOT change after read
        assert_eq!(metadata2.modified, metadata1.modified);
        assert_eq!(metadata2.created, metadata1.created);
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_seek_does_not_update() {
        let mut fs = Fs::new();
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"data").unwrap();
        let metadata1 = fs.fstat(fd).unwrap();
        // Seek within file
        fs.seek(fd, 0, 0).unwrap();
        let metadata2 = fs.fstat(fd).unwrap();
        // modified timestamp should NOT change after seek
        assert_eq!(metadata2.modified, metadata1.modified);
        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod permissions {
    use super::*;

    #[test]
    fn error_write_to_readonly() {
        let mut fs = Fs::new();
        // Create a file first, then open with O_RDONLY flag
        let fd_create = fs.open_path("/readonly.txt").unwrap();
        fs.write(fd_create, b"initial").unwrap();
        fs.close(fd_create).unwrap();
        // Open file with O_RDONLY flag
        let fd = fs.open_path_with_flags("/readonly.txt", O_RDONLY).unwrap();
        // Writing to read-only fd should fail
        let result = fs.write(fd, b"data");
        assert!(matches!(result, Err(FsError::PermissionDenied)));
        fs.close(fd).unwrap();
    }

    #[test]
    fn error_read_from_writeonly() {
        let mut fs = Fs::new();
        // Open file with O_WRONLY flag (need O_CREAT to create)
        let fd = fs
            .open_path_with_flags("/writeonly.txt", O_WRONLY | O_CREAT)
            .unwrap();
        // Write some data
        fs.write(fd, b"data").unwrap();
        // Seek back to beginning
        fs.seek(fd, 0, 0).unwrap();
        // Reading from write-only fd should fail
        let mut buf = [0u8; 10];
        let result = fs.read(fd, &mut buf);
        assert!(matches!(result, Err(FsError::PermissionDenied)));
        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod error_handling {
    use super::*;

    #[test]
    fn error_bad_file_descriptor() {
        let mut fs = Fs::new();
        // Invalid file descriptor should return BadFileDescriptor
        let result = fs.read(999, &mut [0u8; 10]);
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
        let result = fs.write(999, b"data");
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
        let result = fs.seek(999, 0, 0);
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
        let result = fs.fstat(999);
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
        let result = fs.close(999);
        assert!(matches!(result, Err(FsError::BadFileDescriptor)));
    }

    #[test]
    fn error_invalid_arguments() {
        let mut fs = Fs::new();
        // Empty path should return InvalidArgument
        let result = fs.open_path("");
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        let result = fs.mkdir("");
        assert!(matches!(result, Err(FsError::InvalidArgument)));
        let result = fs.stat("");
        assert!(matches!(result, Err(FsError::InvalidArgument)));
    }

    #[test]
    fn error_not_a_directory() {
        let mut fs = Fs::new();
        // Create a file
        let fd = fs.open_path("/file.txt").unwrap();
        fs.write(fd, b"content").unwrap();
        fs.close(fd).unwrap();
        // Trying to create a file inside a file should fail
        let result = fs.open_path("/file.txt/nested");
        assert!(matches!(result, Err(FsError::NotADirectory)));
        // Trying to create a directory inside a file should fail
        let result = fs.mkdir("/file.txt/dir");
        assert!(matches!(result, Err(FsError::NotADirectory)));
    }
}

#[cfg(test)]
mod file_flags_append_trunc {
    use super::*;

    #[test]
    fn success_append_mode() {
        let mut fs = Fs::new();
        // Create a file with initial content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"Hello").unwrap();
        fs.close(fd).unwrap();

        // Open in append mode
        let fd = fs
            .open_path_with_flags("/test.txt", O_WRONLY | O_APPEND)
            .unwrap();

        // Write should append to end, regardless of position
        fs.write(fd, b" World").unwrap();

        // Verify content
        fs.close(fd).unwrap();
        let fd = fs.open_path_with_flags("/test.txt", O_RDONLY).unwrap();
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello World");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_append_multiple_writes() {
        let mut fs = Fs::new();
        // Open in append mode from the start
        let fd = fs
            .open_path_with_flags("/test.txt", O_WRONLY | O_CREAT | O_APPEND)
            .unwrap();

        // Multiple writes should all append
        fs.write(fd, b"First").unwrap();
        fs.write(fd, b"Second").unwrap();
        fs.write(fd, b"Third").unwrap();

        fs.close(fd).unwrap();

        // Verify all writes were appended
        let fd = fs.open_path_with_flags("/test.txt", O_RDONLY).unwrap();
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"FirstSecondThird");
        fs.close(fd).unwrap();
    }

    #[test]
    fn success_append_after_seek() {
        let mut fs = Fs::new();
        let fd = fs
            .open_path_with_flags("/test.txt", O_RDWR | O_CREAT | O_APPEND)
            .unwrap();

        fs.write(fd, b"Hello").unwrap();

        // Seek to beginning (should be ignored for writes in O_APPEND mode)
        fs.seek(fd, 0, 0).unwrap();

        // Write should still append to end
        fs.write(fd, b" World").unwrap();

        // Verify
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello World");

        fs.close(fd).unwrap();
    }

    #[test]
    fn success_trunc_on_open() {
        let mut fs = Fs::new();
        // Create a file with content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"This will be truncated").unwrap();
        fs.close(fd).unwrap();

        // Open with O_TRUNC should clear the file
        let fd = fs
            .open_path_with_flags("/test.txt", O_WRONLY | O_TRUNC)
            .unwrap();

        // Verify file is empty
        fs.close(fd).unwrap();
        let fd = fs.open_path_with_flags("/test.txt", O_RDONLY).unwrap();
        let mut buf = [0u8; 100];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 0);

        fs.close(fd).unwrap();
    }

    #[test]
    fn success_trunc_then_write() {
        let mut fs = Fs::new();
        // Create file with old content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"Old content that is very long").unwrap();
        fs.close(fd).unwrap();

        // Open with O_TRUNC and write new content
        let fd = fs
            .open_path_with_flags("/test.txt", O_RDWR | O_TRUNC)
            .unwrap();
        fs.write(fd, b"New").unwrap();

        // Verify only new content exists
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 50];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"New");

        fs.close(fd).unwrap();
    }

    #[test]
    fn success_append_and_trunc_combined() {
        let mut fs = Fs::new();
        // Create file with content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"Old content").unwrap();
        fs.close(fd).unwrap();

        // O_TRUNC | O_APPEND: truncate first, then append
        let fd = fs
            .open_path_with_flags("/test.txt", O_RDWR | O_TRUNC | O_APPEND)
            .unwrap();

        // File should be empty, writes should append
        fs.write(fd, b"First").unwrap();
        fs.write(fd, b"Second").unwrap();

        fs.seek(fd, 0, 0).unwrap();
        let mut buf = [0u8; 20];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"FirstSecond");

        fs.close(fd).unwrap();
    }

    #[test]
    fn success_trunc_new_file() {
        let mut fs = Fs::new();
        // O_TRUNC on new file (with O_CREAT) should work
        let fd = fs
            .open_path_with_flags("/new.txt", O_RDWR | O_CREAT | O_TRUNC)
            .unwrap();

        fs.write(fd, b"Content").unwrap();
        fs.seek(fd, 0, 0).unwrap();

        let mut buf = [0u8; 10];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Content");

        fs.close(fd).unwrap();
    }

    #[test]
    fn error_trunc_with_rdonly() {
        let mut fs = Fs::new();
        // Create a file with content
        let fd = fs.open_path("/test.txt").unwrap();
        fs.write(fd, b"data").unwrap();
        fs.close(fd).unwrap();

        // O_TRUNC with O_RDONLY should fail (POSIX requires write permission for truncate)
        let result = fs.open_path_with_flags("/test.txt", O_RDONLY | O_TRUNC);
        assert!(matches!(result, Err(FsError::InvalidArgument)));
    }
}
