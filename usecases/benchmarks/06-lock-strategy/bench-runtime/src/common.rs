//! Common benchmark code shared by all lock strategy binaries

use anyhow::Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub const THREAD_COUNTS: [usize; 4] = [1, 4, 8, 16];
pub const OPS_PER_THREAD: usize = 500;
pub const DATA_SIZES: [usize; 3] = [1024, 64 * 1024, 1024 * 1024];

#[derive(Debug, Clone, Copy)]
pub struct BenchConfig {
    pub scenario: &'static str,
    pub file_scope: &'static str,
    #[allow(dead_code)]
    pub description: &'static str,
}

pub const SCENARIOS: [BenchConfig; 5] = [
    BenchConfig {
        scenario: "read",
        file_scope: "same",
        description: "Same file parallel reads",
    },
    BenchConfig {
        scenario: "read",
        file_scope: "different",
        description: "Different files parallel reads",
    },
    BenchConfig {
        scenario: "write",
        file_scope: "same",
        description: "Same file parallel writes",
    },
    BenchConfig {
        scenario: "write",
        file_scope: "different",
        description: "Different files parallel writes",
    },
    BenchConfig {
        scenario: "mixed",
        file_scope: "same",
        description: "Mixed read/write same file",
    },
];

#[derive(Debug)]
pub struct BenchResult {
    pub total_ops: usize,
    pub duration_ms: f64,
    pub throughput: f64,
    pub error_count: usize,
}

#[derive(Debug)]
pub struct CorrectnessResult {
    pub expected_lines: usize,
    pub actual_lines: usize,
    pub errors: usize,
}

impl CorrectnessResult {
    pub fn integrity_percent(&self) -> f64 {
        if self.expected_lines == 0 {
            return 100.0;
        }
        (self.actual_lines as f64 / self.expected_lines as f64) * 100.0
    }

    pub fn is_pass(&self) -> bool {
        self.actual_lines == self.expected_lines && self.errors == 0
    }
}

/// Trait for VFS operations needed by benchmarks
pub trait VfsOps: Send + Sync {
    fn mkdir(&self, path: &str) -> Result<(), anyhow::Error>;
    fn open_path(&self, path: &str) -> Result<u32, anyhow::Error>;
    fn write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error>;
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, anyhow::Error>;
    fn append_write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error>;
    fn close(&self, fd: u32) -> Result<(), anyhow::Error>;
}

/// Setup test data in the VFS
pub fn setup_test_data<V: VfsOps>(fs: &V, thread_count: usize, data_size: usize) -> Result<()> {
    fs.mkdir("/bench")?;
    fs.mkdir("/bench/shared")?;
    fs.mkdir("/bench/files")?;

    let content = vec![b'D'; data_size.max(4096)];
    for name in ["data.txt", "write_target.txt", "mixed.txt"] {
        let path = format!("/bench/shared/{}", name);
        let fd = fs.open_path(&path)?;
        fs.write(fd, &content)?;
        fs.close(fd)?;
    }

    let thread_content = vec![b'T'; data_size.max(1024)];
    for i in 0..thread_count {
        let path = format!("/bench/files/thread_{}.txt", i);
        let fd = fs.open_path(&path)?;
        fs.write(fd, &thread_content)?;
        fs.close(fd)?;
    }

    Ok(())
}

/// Run correctness verification using append_write (atomic operation)
pub fn run_correctness_check<V: VfsOps + 'static>(fs: Arc<V>) -> Result<CorrectnessResult> {
    fs.mkdir("/verify")?;

    let fd = fs.open_path("/verify/append_test.txt")?;
    fs.close(fd)?;

    let num_threads = 8;
    let appends_per_thread = 50;
    let error_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let vfs = Arc::clone(&fs);
        let errors = Arc::clone(&error_count);

        handles.push(std::thread::spawn(move || {
            for i in 0..appends_per_thread {
                let fd = match vfs.open_path("/verify/append_test.txt") {
                    Ok(fd) => fd,
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };

                // Use append_write (atomic) instead of seek+write (non-atomic)
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

    let fd = fs.open_path("/verify/append_test.txt")?;
    let mut content = vec![0u8; 1024 * 1024];
    let bytes_read = fs.read(fd, &mut content)?;
    fs.close(fd)?;

    let content = String::from_utf8_lossy(&content[..bytes_read]);
    let line_count = content.lines().count();
    let expected_lines = num_threads * appends_per_thread;

    Ok(CorrectnessResult {
        expected_lines,
        actual_lines: line_count,
        errors: error_count.load(Ordering::Relaxed),
    })
}

/// Print CSV header
pub fn print_csv_header() {
    println!("strategy,scenario,file_scope,threads,data_size,total_ops,duration_ms,throughput_ops_sec,errors,data_integrity");
}

/// Measure benchmark duration
pub struct BenchTimer {
    start: Instant,
}

impl BenchTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    pub fn throughput(&self, total_ops: usize) -> f64 {
        total_ops as f64 / self.start.elapsed().as_secs_f64()
    }
}
