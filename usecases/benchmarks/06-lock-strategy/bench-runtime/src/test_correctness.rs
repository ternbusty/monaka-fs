//! Quick correctness test for append_write across different locking strategies

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

fn test_append_correctness<F, V>(name: &str, create_vfs: F)
where
    F: Fn() -> Arc<V>,
    V: VfsOps + Send + Sync + 'static,
{
    let fs = create_vfs();

    fs.mkdir("/test").unwrap();
    let fd = fs.open_path("/test/append.txt").unwrap();
    fs.close(fd).unwrap();

    let num_threads = 8;
    let appends_per_thread = 50;
    let error_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let vfs = Arc::clone(&fs);
        let errors = Arc::clone(&error_count);

        handles.push(thread::spawn(move || {
            for i in 0..appends_per_thread {
                let fd = match vfs.open_path("/test/append.txt") {
                    Ok(fd) => fd,
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };

                let marker = format!("T{}I{}\n", thread_id, i);
                if let Err(_) = vfs.append_write(fd, marker.as_bytes()) {
                    errors.fetch_add(1, Ordering::Relaxed);
                }

                let _ = vfs.close(fd);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let fd = fs.open_path("/test/append.txt").unwrap();
    let mut content = vec![0u8; 1024 * 1024];
    let bytes_read = fs.read(fd, &mut content).unwrap();
    fs.close(fd).unwrap();

    let content = String::from_utf8_lossy(&content[..bytes_read]);
    let line_count = content.lines().count();
    let expected_lines = num_threads * appends_per_thread;
    let errors = error_count.load(Ordering::Relaxed);
    let integrity = (line_count as f64 / expected_lines as f64) * 100.0;

    if line_count == expected_lines && errors == 0 {
        println!("{}: PASS - {}/{} lines (100.0%)", name, line_count, expected_lines);
    } else {
        println!(
            "{}: FAIL - {}/{} lines ({:.1}%), {} errors",
            name, line_count, expected_lines, integrity, errors
        );
    }
}

trait VfsOps: Send + Sync {
    fn mkdir(&self, path: &str) -> Result<(), anyhow::Error>;
    fn open_path(&self, path: &str) -> Result<u32, anyhow::Error>;
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, anyhow::Error>;
    fn append_write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error>;
    fn close(&self, fd: u32) -> Result<(), anyhow::Error>;
}

impl VfsOps for fs_core::Fs {
    fn mkdir(&self, path: &str) -> Result<(), anyhow::Error> {
        self.mkdir(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn open_path(&self, path: &str) -> Result<u32, anyhow::Error> {
        self.open_path(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, anyhow::Error> {
        self.read(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn append_write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error> {
        self.append_write(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn close(&self, fd: u32) -> Result<(), anyhow::Error> {
        self.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
}

impl VfsOps for fs_global::Fs {
    fn mkdir(&self, path: &str) -> Result<(), anyhow::Error> {
        self.mkdir(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn open_path(&self, path: &str) -> Result<u32, anyhow::Error> {
        self.open_path(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, anyhow::Error> {
        self.read(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn append_write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error> {
        self.append_write(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn close(&self, fd: u32) -> Result<(), anyhow::Error> {
        self.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
}

impl VfsOps for fs_unsafe::Fs {
    fn mkdir(&self, path: &str) -> Result<(), anyhow::Error> {
        self.mkdir(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn open_path(&self, path: &str) -> Result<u32, anyhow::Error> {
        self.open_path(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, anyhow::Error> {
        self.read(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn append_write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error> {
        self.append_write(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn close(&self, fd: u32) -> Result<(), anyhow::Error> {
        self.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
}

fn main() {
    println!("=== Correctness Test (append_write) ===\n");

    println!("Testing lock-fine (fs-core with DashMap + RwLock)...");
    test_append_correctness("lock-fine", || Arc::new(fs_core::Fs::new()));

    println!("\nTesting lock-global (fs-global with single RwLock)...");
    test_append_correctness("lock-global", || Arc::new(fs_global::Fs::new()));

    println!("\nTesting lock-none (fs-unsafe with no locking)...");
    test_append_correctness("lock-none", || Arc::new(fs_unsafe::Fs::new()));

    println!("\n=== Test Complete ===");
}
