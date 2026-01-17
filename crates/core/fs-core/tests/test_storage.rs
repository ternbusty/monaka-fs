use fs_core::{BLOCK_SIZE, Fs};

#[cfg(test)]
mod block_storage {
    use super::*;

    #[test]
    fn success_large_file_multiple_blocks() {
        let fs = Fs::new();
        let fd = fs.open_path("/large.bin").unwrap();

        // Write data that spans multiple blocks
        let large_data: Vec<u8> = (0..BLOCK_SIZE * 2 + 100).map(|i| (i % 256) as u8).collect();

        let written = fs.write(fd, &large_data).unwrap();
        assert_eq!(written, large_data.len());

        // Seek to beginning and read it back
        fs.seek(fd, 0, 0).unwrap();
        let mut read_buf = vec![0u8; large_data.len()];
        let read = fs.read(fd, &mut read_buf).unwrap();
        assert_eq!(read, large_data.len());
        assert_eq!(read_buf, large_data);

        fs.close(fd).unwrap();
    }

    #[test]
    fn success_boundary_operations() {
        let fs = Fs::new();
        let fd = fs.open_path("/boundary.txt").unwrap();

        // Write data that ends exactly at block boundary
        let data1 = vec![1u8; BLOCK_SIZE];
        fs.write(fd, &data1).unwrap();

        // Write more data starting at block boundary
        let data2 = vec![2u8; 100];
        fs.write(fd, &data2).unwrap();

        // Read across block boundary
        fs.seek(fd, (BLOCK_SIZE - 50) as i64, 0).unwrap();
        let mut buf = vec![0u8; 100];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 100);
        assert_eq!(&buf[..50], &vec![1u8; 50]);
        assert_eq!(&buf[50..], &vec![2u8; 50]);

        fs.close(fd).unwrap();
    }
}

#[cfg(test)]
mod sparse_files {
    use super::*;

    #[test]
    fn success_with_gaps() {
        let fs = Fs::new();
        let fd = fs.open_path("/sparse.dat").unwrap();

        // Write at offset 0
        fs.write(fd, b"hello").unwrap();

        // Seek to a large offset (creating a sparse file)
        fs.seek(fd, BLOCK_SIZE as i64 * 10, 0).unwrap();

        // Write at the large offset
        fs.write(fd, b"world").unwrap();

        // Read from the beginning
        fs.seek(fd, 0, 0).unwrap();
        let mut buf = vec![0u8; 5];
        let n = fs.read(fd, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello");

        // Read from the sparse region (should be zeros)
        fs.seek(fd, 100, 0).unwrap();
        let mut sparse_buf = vec![0u8; 10];
        let n = fs.read(fd, &mut sparse_buf).unwrap();
        assert_eq!(n, 10);
        assert_eq!(sparse_buf, vec![0u8; 10]);

        // Read from the large offset
        fs.seek(fd, BLOCK_SIZE as i64 * 10, 0).unwrap();
        let mut buf2 = vec![0u8; 5];
        let n = fs.read(fd, &mut buf2).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf2, b"world");

        fs.close(fd).unwrap();
    }
}
