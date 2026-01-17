//! Benchmark: s3fs-fuse vs vfs-host S3 Sync
//!
//! This benchmark measures write and read performance with S3 persistence.
//! Compares:
//! - s3fs-fuse: Direct S3 mount via FUSE (synchronous S3 operations)
//! - vfs-host: In-memory VFS with background S3 sync

use std::fs::{self};
use std::time::Instant;

const ITERATIONS: usize = 5;

fn main() {
    let mode = std::env::var("BENCH_MODE").unwrap_or_else(|_| "vfs-host".to_string());

    println!("=== Benchmark: {} ===", mode);
    println!("Working directory: /data");
    println!();

    // Ensure /data directory exists
    match fs::create_dir("/data") {
        Ok(_) => println!("[INIT] Created /data directory"),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            println!("[INIT] /data directory already exists")
        }
        Err(e) => {
            println!("[ERROR] Failed to create /data: {}", e);
            return;
        }
    }

    let file_sizes: &[(usize, &str)] = &[
        (1 * 1024, "1KB"),
        (10 * 1024, "10KB"),
        (100 * 1024, "100KB"),
        (1 * 1024 * 1024, "1MB"),
        (10 * 1024 * 1024, "10MB"),
    ];

    let total_start = Instant::now();

    for &(size, label) in file_sizes {
        println!("--- File Size: {} ---", label);
        run_benchmark(size, label);
        println!();
    }

    let total_elapsed = total_start.elapsed();
    println!(
        "[TOTAL] All operations completed in {:.3}ms",
        total_elapsed.as_secs_f64() * 1000.0
    );

    println!();
    println!("=== Benchmark Complete ===");
}

fn run_benchmark(file_size: usize, label: &str) {
    let data = generate_test_data(file_size);

    // Sequential Write - use different file for each iteration
    let write_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/data/benchmark_write_{}_{}.dat", label, i);
            let start = Instant::now();
            fs::write(&path, &data).expect("Failed to write file");
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            elapsed
        })
        .collect();

    // Cleanup write test files
    for i in 0..ITERATIONS {
        let path = format!("/data/benchmark_write_{}_{}.dat", label, i);
        let _ = fs::remove_file(&path);
    }

    let write_ms = median(&write_durations);
    let write_throughput = (file_size as f64 / (1024.0 * 1024.0)) / (write_ms / 1000.0);
    println!(
        "[RESULT] seq_write,{},{:.3},{:.2}",
        label, write_ms, write_throughput
    );

    // Sequential Read - create files first, then read each once
    for i in 0..ITERATIONS {
        let path = format!("/data/benchmark_read_{}_{}.dat", label, i);
        fs::write(&path, &data).expect("Failed to write file for read test");
    }
    let read_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/data/benchmark_read_{}_{}.dat", label, i);
            let start = Instant::now();
            let _ = fs::read(&path).expect("Failed to read file");
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();

    // Cleanup read test files
    for i in 0..ITERATIONS {
        let path = format!("/data/benchmark_read_{}_{}.dat", label, i);
        let _ = fs::remove_file(&path);
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
