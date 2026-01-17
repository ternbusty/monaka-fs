//! VFS Lock Comparison Benchmark
//!
//! Compares concurrent performance between:
//! 1. Arc<Mutex<Fs>> - Traditional external lock (all ops serialized)
//! 2. Arc<Fs> - DashMap-based fine-grained locking (concurrent access)
//!
//! Run with:
//!   cargo run --release

use anyhow::Result;
use fs_core::Fs;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const NUM_THREADS: usize = 8;
const OPS_PER_THREAD: usize = 10_000;

// fs-core open flags
const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_CREAT: u32 = 0o100;
const O_TRUNC: u32 = 0o1000;

fn main() -> Result<()> {
    println!("=== VFS Lock Comparison Benchmark ===");
    println!("Threads: {}, Ops per thread: {}", NUM_THREADS, OPS_PER_THREAD);
    println!();

    // ========================================
    // Method 1: Arc<Mutex<Fs>> (Traditional)
    // ========================================
    println!("========================================");
    println!("Method 1: Arc<Mutex<Fs>> (External Lock)");
    println!("  All operations are serialized");
    println!("========================================");
    println!();

    let fs_mutex = Arc::new(Mutex::new(Fs::new()));
    setup_test_data_mutex(&fs_mutex)?;

    println!("Scenario 1: Concurrent stat() (read-only)");
    let mutex_stat = bench_concurrent_stat_mutex(&fs_mutex)?;
    print_results(&mutex_stat);

    println!("Scenario 2: Mixed workload (70% stat, 30% mkdir/rmdir)");
    let mutex_mixed = bench_mixed_workload_mutex(&fs_mutex)?;
    print_results(&mutex_mixed);

    println!("Scenario 3: Concurrent read - SAME file");
    let mutex_read = bench_concurrent_read_mutex(&fs_mutex)?;
    print_results(&mutex_read);

    println!("Scenario 4: File write operations");
    let mutex_io = bench_file_io_mutex(&fs_mutex)?;
    print_results(&mutex_io);

    // ========================================
    // Method 2: Arc<Fs> (DashMap Fine-Grained)
    // ========================================
    println!("========================================");
    println!("Method 2: Arc<Fs> (DashMap Fine-Grained)");
    println!("  Internal locking, concurrent access");
    println!("========================================");
    println!();

    let fs_dashmap = Arc::new(Fs::new());
    setup_test_data_dashmap(&fs_dashmap)?;

    println!("Scenario 1: Concurrent stat() (read-only)");
    let dashmap_stat = bench_concurrent_stat_dashmap(&fs_dashmap)?;
    print_results(&dashmap_stat);

    println!("Scenario 2: Mixed workload (70% stat, 30% mkdir/rmdir)");
    let dashmap_mixed = bench_mixed_workload_dashmap(&fs_dashmap)?;
    print_results(&dashmap_mixed);

    println!("Scenario 3: Concurrent read - SAME file");
    let dashmap_read = bench_concurrent_read_dashmap(&fs_dashmap)?;
    print_results(&dashmap_read);

    println!("Scenario 4: File write operations");
    let dashmap_io = bench_file_io_dashmap(&fs_dashmap)?;
    print_results(&dashmap_io);

    // ========================================
    // Comparison Summary
    // ========================================
    println!("========================================");
    println!("COMPARISON SUMMARY");
    println!("========================================");
    println!();
    println!("{:<25} {:>15} {:>15} {:>10}", "Scenario", "Mutex", "DashMap", "Speedup");
    println!("{:-<25} {:-<15} {:-<15} {:-<10}", "", "", "", "");

    print_comparison("stat() read-only", &mutex_stat, &dashmap_stat);
    print_comparison("Mixed workload", &mutex_mixed, &dashmap_mixed);
    print_comparison("Concurrent read", &mutex_read, &dashmap_read);
    print_comparison("File write", &mutex_io, &dashmap_io);

    Ok(())
}

struct BenchResult {
    total_ops: usize,
    duration: Duration,
    latencies: Vec<Duration>,
}

impl BenchResult {
    fn throughput(&self) -> f64 {
        self.total_ops as f64 / self.duration.as_secs_f64()
    }

    fn p50_latency(&self) -> Duration {
        let mut sorted = self.latencies.clone();
        sorted.sort();
        sorted[sorted.len() / 2]
    }

    fn p99_latency(&self) -> Duration {
        let mut sorted = self.latencies.clone();
        sorted.sort();
        sorted[(sorted.len() as f64 * 0.99) as usize]
    }
}

fn print_results(result: &BenchResult) {
    println!(
        "  Throughput: {:.0} ops/sec",
        result.throughput()
    );
    println!(
        "  Latency p50: {:?}, p99: {:?}",
        result.p50_latency(),
        result.p99_latency()
    );
    println!();
}

fn print_comparison(name: &str, mutex: &BenchResult, dashmap: &BenchResult) {
    let mutex_tp = mutex.throughput();
    let dashmap_tp = dashmap.throughput();
    let speedup = dashmap_tp / mutex_tp;
    println!(
        "{:<25} {:>12.0} ops {:>12.0} ops {:>9.2}x",
        name, mutex_tp, dashmap_tp, speedup
    );
}

// ========================================
// Arc<Mutex<Fs>> implementations
// ========================================

fn setup_test_data_mutex(fs: &Arc<Mutex<Fs>>) -> Result<()> {
    let fs = fs.lock().unwrap();
    fs.mkdir("/bench").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/dir1").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/dir2").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/mixed").map_err(|e| anyhow::anyhow!("{:?}", e))?;

    for i in 0..10 {
        let path = format!("/bench/file{}.txt", i);
        let fd = fs
            .open_path_with_flags(&path, O_WRONLY | O_CREAT | O_TRUNC)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let content = format!("Test file content {}", i);
        fs.write(fd, content.as_bytes())
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }
    Ok(())
}

fn bench_concurrent_stat_mutex(fs: &Arc<Mutex<Fs>>) -> Result<BenchResult> {
    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));

    for _ in 0..NUM_THREADS {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(OPS_PER_THREAD);

            for i in 0..OPS_PER_THREAD {
                let path = format!("/bench/file{}.txt", i % 10);
                let op_start = Instant::now();

                // Must acquire lock for every operation
                let fs = fs.lock().unwrap();
                let _ = fs.stat(&path);
                drop(fs);

                local_latencies.push(op_start.elapsed());
            }

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * OPS_PER_THREAD,
        duration,
        latencies,
    })
}

fn bench_mixed_workload_mutex(fs: &Arc<Mutex<Fs>>) -> Result<BenchResult> {
    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));

    for thread_id in 0..NUM_THREADS {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(OPS_PER_THREAD);

            for i in 0..OPS_PER_THREAD {
                let op_start = Instant::now();

                let fs = fs.lock().unwrap();
                if i % 10 < 7 {
                    let path = format!("/bench/file{}.txt", i % 10);
                    let _ = fs.stat(&path);
                } else {
                    let dir_path = format!("/bench/mixed/t{}_{}", thread_id, i);
                    let _ = fs.mkdir(&dir_path);
                    let _ = fs.rmdir(&dir_path);
                }
                drop(fs);

                local_latencies.push(op_start.elapsed());
            }

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * OPS_PER_THREAD,
        duration,
        latencies,
    })
}

fn bench_concurrent_read_mutex(fs: &Arc<Mutex<Fs>>) -> Result<BenchResult> {
    let path = "/bench/read/shared.txt";
    {
        let fs = fs.lock().unwrap();
        let _ = fs.mkdir("/bench/read");
        let fd = fs
            .open_path_with_flags(path, O_WRONLY | O_CREAT | O_TRUNC)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let content = "Test content for shared file - all threads read this same data!";
        fs.write(fd, content.as_bytes()).map_err(|e| anyhow::anyhow!("{:?}", e))?;
        fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }

    let mut thread_fds: Vec<u32> = Vec::with_capacity(NUM_THREADS);
    {
        let fs = fs.lock().unwrap();
        for _ in 0..NUM_THREADS {
            let fd = fs
                .open_path_with_flags(path, O_RDONLY)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
            thread_fds.push(fd);
        }
    }

    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));
    let reduced_ops = OPS_PER_THREAD / 10;

    for fd in thread_fds.into_iter() {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(reduced_ops);

            for _ in 0..reduced_ops {
                let op_start = Instant::now();
                let mut buf = [0u8; 64];

                let fs = fs.lock().unwrap();
                let _ = fs.seek(fd, 0, 0);
                let _ = fs.read(fd, &mut buf);
                drop(fs);

                local_latencies.push(op_start.elapsed());
            }

            let fs = fs.lock().unwrap();
            let _ = fs.close(fd);
            drop(fs);

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * reduced_ops,
        duration,
        latencies,
    })
}

fn bench_file_io_mutex(fs: &Arc<Mutex<Fs>>) -> Result<BenchResult> {
    {
        let fs = fs.lock().unwrap();
        fs.mkdir("/bench/io").map_err(|e| anyhow::anyhow!("{:?}", e))?;
        for i in 0..NUM_THREADS {
            let path = format!("/bench/io/thread{}.txt", i);
            let fd = fs
                .open_path_with_flags(&path, O_WRONLY | O_CREAT | O_TRUNC)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
            fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
        }
    }

    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));
    let reduced_ops = OPS_PER_THREAD / 10;

    for thread_id in 0..NUM_THREADS {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(reduced_ops);
            let path = format!("/bench/io/thread{}.txt", thread_id);

            for i in 0..reduced_ops {
                let op_start = Instant::now();
                let content = format!("Data from thread {} iteration {}", thread_id, i);

                let fs = fs.lock().unwrap();
                let fd = fs.open_path_with_flags(&path, O_WRONLY | O_TRUNC).unwrap();
                let _ = fs.write(fd, content.as_bytes());
                fs.close(fd).unwrap();
                drop(fs);

                local_latencies.push(op_start.elapsed());
            }

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * reduced_ops,
        duration,
        latencies,
    })
}

// ========================================
// Arc<Fs> (DashMap) implementations
// ========================================

fn setup_test_data_dashmap(fs: &Arc<Fs>) -> Result<()> {
    fs.mkdir("/bench").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/dir1").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/dir2").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/mixed").map_err(|e| anyhow::anyhow!("{:?}", e))?;

    for i in 0..10 {
        let path = format!("/bench/file{}.txt", i);
        let fd = fs
            .open_path_with_flags(&path, O_WRONLY | O_CREAT | O_TRUNC)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let content = format!("Test file content {}", i);
        fs.write(fd, content.as_bytes())
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }
    Ok(())
}

fn bench_concurrent_stat_dashmap(fs: &Arc<Fs>) -> Result<BenchResult> {
    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));

    for _ in 0..NUM_THREADS {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(OPS_PER_THREAD);

            for i in 0..OPS_PER_THREAD {
                let path = format!("/bench/file{}.txt", i % 10);
                let op_start = Instant::now();

                // No external lock needed - direct access
                let _ = fs.stat(&path);

                local_latencies.push(op_start.elapsed());
            }

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * OPS_PER_THREAD,
        duration,
        latencies,
    })
}

fn bench_mixed_workload_dashmap(fs: &Arc<Fs>) -> Result<BenchResult> {
    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));

    for thread_id in 0..NUM_THREADS {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(OPS_PER_THREAD);

            for i in 0..OPS_PER_THREAD {
                let op_start = Instant::now();

                if i % 10 < 7 {
                    let path = format!("/bench/file{}.txt", i % 10);
                    let _ = fs.stat(&path);
                } else {
                    let dir_path = format!("/bench/mixed/t{}_{}", thread_id, i);
                    let _ = fs.mkdir(&dir_path);
                    let _ = fs.rmdir(&dir_path);
                }

                local_latencies.push(op_start.elapsed());
            }

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * OPS_PER_THREAD,
        duration,
        latencies,
    })
}

fn bench_concurrent_read_dashmap(fs: &Arc<Fs>) -> Result<BenchResult> {
    let path = "/bench/read/shared.txt";
    {
        let _ = fs.mkdir("/bench/read");
        let fd = fs
            .open_path_with_flags(path, O_WRONLY | O_CREAT | O_TRUNC)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let content = "Test content for shared file - all threads read this same data!";
        fs.write(fd, content.as_bytes()).map_err(|e| anyhow::anyhow!("{:?}", e))?;
        fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }

    let mut thread_fds: Vec<u32> = Vec::with_capacity(NUM_THREADS);
    {
        for _ in 0..NUM_THREADS {
            let fd = fs
                .open_path_with_flags(path, O_RDONLY)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
            thread_fds.push(fd);
        }
    }

    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));
    let reduced_ops = OPS_PER_THREAD / 10;

    for fd in thread_fds.into_iter() {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(reduced_ops);

            for _ in 0..reduced_ops {
                let op_start = Instant::now();
                let mut buf = [0u8; 64];

                let _ = fs.seek(fd, 0, 0);
                let _ = fs.read(fd, &mut buf);

                local_latencies.push(op_start.elapsed());
            }

            let _ = fs.close(fd);

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * reduced_ops,
        duration,
        latencies,
    })
}

fn bench_file_io_dashmap(fs: &Arc<Fs>) -> Result<BenchResult> {
    {
        fs.mkdir("/bench/io").map_err(|e| anyhow::anyhow!("{:?}", e))?;
        for i in 0..NUM_THREADS {
            let path = format!("/bench/io/thread{}.txt", i);
            let fd = fs
                .open_path_with_flags(&path, O_WRONLY | O_CREAT | O_TRUNC)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
            fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
        }
    }

    let start = Instant::now();
    let mut handles = vec![];
    let all_latencies = Arc::new(Mutex::new(Vec::new()));
    let reduced_ops = OPS_PER_THREAD / 10;

    for thread_id in 0..NUM_THREADS {
        let fs = Arc::clone(fs);
        let latencies = Arc::clone(&all_latencies);

        handles.push(thread::spawn(move || {
            let mut local_latencies = Vec::with_capacity(reduced_ops);
            let path = format!("/bench/io/thread{}.txt", thread_id);

            for i in 0..reduced_ops {
                let op_start = Instant::now();
                let content = format!("Data from thread {} iteration {}", thread_id, i);

                let fd = fs.open_path_with_flags(&path, O_WRONLY | O_TRUNC).unwrap();
                let _ = fs.write(fd, content.as_bytes());
                fs.close(fd).unwrap();

                local_latencies.push(op_start.elapsed());
            }

            latencies.lock().unwrap().extend(local_latencies);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let latencies = Arc::try_unwrap(all_latencies)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * reduced_ops,
        duration,
        latencies,
    })
}
