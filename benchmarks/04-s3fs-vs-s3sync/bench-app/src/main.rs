//! Benchmark: s3fs-fuse vs S3 Sync
//!
//! This benchmark measures write and read performance with S3 persistence.
//! For Halycon, writes go to in-memory VFS and sync happens at session end.

use std::fs::{self};
use std::time::Instant;

const ITERATIONS: usize = 5;

fn main() {
    println!("=== Benchmark: Halycon S3 Sync ===");
    println!("Working directory: /data");
    println!("Note: Data syncs to S3 at session end");
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
        (1 * 1024 * 1024, "1MB"),
        (10 * 1024 * 1024, "10MB"),
        (100 * 1024 * 1024, "100MB"),
    ];

    let total_start = Instant::now();

    for &(size, label) in file_sizes {
        println!("--- File Size: {} ---", label);
        run_benchmark(size, label);
        println!();
    }

    let total_elapsed = total_start.elapsed();
    println!(
        "[TOTAL] In-memory operations completed in {:.3}ms",
        total_elapsed.as_secs_f64() * 1000.0
    );
    println!("[NOTE] S3 sync will occur when session ends");

    println!();
    println!("=== Benchmark Complete ===");
}

fn run_benchmark(file_size: usize, label: &str) {
    let data = generate_test_data(file_size);

    // Sequential Write (to in-memory VFS) - use different file for each iteration
    let write_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/data/benchmark_write_{}_{}.dat", label, i);
            let start = Instant::now();
            fs::write(&path, &data).expect("Failed to write file");
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let _ = fs::remove_file(&path);
            elapsed
        })
        .collect();
    let write_ms = median(&write_durations);
    let write_throughput = (file_size as f64 / (1024.0 * 1024.0)) / (write_ms / 1000.0);
    println!(
        "[RESULT] seq_write,{},{:.3},{:.2}",
        label, write_ms, write_throughput
    );

    // Sequential Read (from in-memory VFS) - create files first, then read each once
    for i in 0..ITERATIONS {
        let path = format!("/data/benchmark_read_{}_{}.dat", label, i);
        fs::write(&path, &data).expect("Failed to write file for read test");
    }
    let read_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/data/benchmark_read_{}_{}.dat", label, i);
            let start = Instant::now();
            let _ = fs::read(&path).expect("Failed to read file");
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let _ = fs::remove_file(&path);
            elapsed
        })
        .collect();
    let read_ms = median(&read_durations);
    let read_throughput = (file_size as f64 / (1024.0 * 1024.0)) / (read_ms / 1000.0);
    println!(
        "[RESULT] seq_read,{},{:.3},{:.2}",
        label, read_ms, read_throughput
    );

    // Create a final file for S3 sync demonstration
    let final_path = format!("/data/benchmark_{}.dat", label);
    fs::write(&final_path, &data).expect("Failed to write final file for S3 sync");
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
