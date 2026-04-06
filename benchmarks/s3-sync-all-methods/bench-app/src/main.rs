//! Benchmark: S3 Sync - All Methods Comparison
//!
//! This benchmark measures write and read performance with S3 persistence.
//! Compares:
//! - s3fs-fuse: Direct S3 mount via FUSE
//! - vfs-host: Host trait implementation with S3 sync
//! - wac-plug: WASM composition with vfs-adapter
//! - rpc: RPC-based VFS with vfs-rpc-server
//!
//! All VFS implementations run in S3 passthrough mode:
//! - VFS_SYNC_MODE=realtime (immediate S3 sync on write)
//! - VFS_READ_MODE=s3 (read-through from S3)
//! - VFS_METADATA_MODE=s3 (HEAD request on open)

use std::fs::{self};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const ITERATIONS: usize = 5;

/// Simple pseudo-random number generator (xorshift)
struct Rng {
    state: u64,
}

impl Rng {
    fn new() -> Self {
        // Seed from current time
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        Self { state: seed ^ 0xdeadbeef }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn random_id(&mut self) -> String {
        format!("{:016x}", self.next())
    }
}

fn main() {
    let method = std::env::var("BENCH_METHOD").unwrap_or_else(|_| "unknown".to_string());

    eprintln!("=== Benchmark: {} ===", method);
    eprintln!("Working directory: /data");
    eprintln!();

    // Ensure /data directory exists
    match fs::create_dir("/data") {
        Ok(_) => eprintln!("[INIT] Created /data directory"),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            eprintln!("[INIT] /data directory already exists")
        }
        Err(e) => {
            eprintln!("[ERROR] Failed to create /data: {}", e);
            return;
        }
    }

    let mut rng = Rng::new();

    // Warmup: perform MORE dummy operations to fully initialize s3fs
    // s3fs has significant first-operation latency even after initial warmup
    eprintln!("[WARMUP] Performing warmup operations...");
    let warmup_sizes = [1024, 10240, 102400, 1048576]; // 1KB, 10KB, 100KB, 1MB
    for size in warmup_sizes {
        let warmup_data = vec![0u8; size];
        // Do 5 iterations per size to ensure s3fs is fully warmed up
        for _ in 0..5 {
            let path = format!("/data/warmup_{}.dat", rng.random_id());
            fs::write(&path, &warmup_data).ok();
            fs::read(&path).ok();
            fs::remove_file(&path).ok();
        }
    }
    eprintln!("[WARMUP] Done");
    eprintln!();

    // File sizes to benchmark (shuffled to avoid ordering bias)
    let mut file_sizes: Vec<(usize, &str)> = vec![
        (1 * 1024, "1KB"),
        (10 * 1024, "10KB"),
        (100 * 1024, "100KB"),
        (1 * 1024 * 1024, "1MB"),
    ];

    // Fisher-Yates shuffle
    for i in (1..file_sizes.len()).rev() {
        let j = (rng.next() as usize) % (i + 1);
        file_sizes.swap(i, j);
    }

    eprintln!("[ORDER] Testing in order: {:?}", file_sizes.iter().map(|(_, l)| *l).collect::<Vec<_>>());

    let total_start = Instant::now();

    for &(size, label) in &file_sizes {
        eprintln!("--- File Size: {} ---", label);
        run_benchmark(size, label, &mut rng);
        eprintln!();
    }

    let total_elapsed = total_start.elapsed();
    eprintln!(
        "[TOTAL] All operations completed in {:.3}ms",
        total_elapsed.as_secs_f64() * 1000.0
    );

    eprintln!();
    eprintln!("=== Benchmark Complete ===");
}

fn run_benchmark(file_size: usize, label: &str, rng: &mut Rng) {
    let data = generate_test_data(file_size);

    // Sequential Write - use random file name for each iteration
    let mut write_paths = Vec::new();
    let write_durations: Vec<f64> = (0..ITERATIONS)
        .map(|_| {
            let path = format!("/data/bench_w_{}_{}.dat", label, rng.random_id());
            let start = Instant::now();
            fs::write(&path, &data).expect("Failed to write file");
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            write_paths.push(path);
            elapsed
        })
        .collect();

    // Cleanup write test files
    for path in &write_paths {
        let _ = fs::remove_file(path);
    }

    let write_ms = median(&write_durations);
    let write_throughput = (file_size as f64 / (1024.0 * 1024.0)) / (write_ms / 1000.0);
    println!(
        "[RESULT] seq_write,{},{:.3},{:.2}",
        label, write_ms, write_throughput
    );

    // Sequential Read - create files first with random names, then read each once
    let mut read_paths = Vec::new();
    for _ in 0..ITERATIONS {
        let path = format!("/data/bench_r_{}_{}.dat", label, rng.random_id());
        fs::write(&path, &data).expect("Failed to write file for read test");
        read_paths.push(path);
    }

    let read_durations: Vec<f64> = read_paths
        .iter()
        .map(|path| {
            let start = Instant::now();
            let _ = fs::read(path).expect("Failed to read file");
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();

    // Cleanup read test files
    for path in &read_paths {
        let _ = fs::remove_file(path);
    }

    let read_ms = median(&read_durations);
    let read_throughput = (file_size as f64 / (1024.0 * 1024.0)) / (read_ms / 1000.0);
    println!(
        "[RESULT] seq_read,{},{:.3},{:.2}",
        label, read_ms, read_throughput
    );
}

fn generate_test_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    let pattern: Vec<u8> = (0..=255u8).collect();

    while data.len() < size {
        let remaining = size - data.len();
        let chunk_size = remaining.min(pattern.len());
        data.extend_from_slice(&pattern[..chunk_size]);
    }

    data
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[sorted.len() / 2]
}
