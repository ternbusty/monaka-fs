//! Benchmark WASM app for VFS concurrent access testing
//!
//! Performs file operations based on environment variables:
//! - BENCH_THREAD_ID: Thread identifier
//! - BENCH_OPS: Number of operations to perform
//! - BENCH_SCENARIO: "stat", "mixed", "read", or "write"

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

fn main() {
    let thread_id: usize = std::env::var("BENCH_THREAD_ID")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0);

    let ops: usize = std::env::var("BENCH_OPS")
        .unwrap_or_else(|_| "1000".to_string())
        .parse()
        .unwrap_or(1000);

    let scenario = std::env::var("BENCH_SCENARIO")
        .unwrap_or_else(|_| "stat".to_string());

    match scenario.as_str() {
        "stat" => run_stat_benchmark(thread_id, ops),
        "mixed" => run_mixed_benchmark(thread_id, ops),
        "read" => run_read_benchmark(thread_id, ops),
        "write" => run_write_benchmark(thread_id, ops),
        _ => {
            eprintln!("Unknown scenario: {}", scenario);
        }
    }
}

/// Scenario 1: Concurrent stat() operations (read-only metadata access)
fn run_stat_benchmark(_thread_id: usize, ops: usize) {
    for i in 0..ops {
        let path = format!("/bench/file{}.txt", i % 10);
        let _ = fs::metadata(&path);
    }
}

/// Scenario 2: Mixed workload (70% stat, 30% mkdir/rmdir)
fn run_mixed_benchmark(thread_id: usize, ops: usize) {
    for i in 0..ops {
        if i % 10 < 7 {
            // 70% read operation: stat
            let path = format!("/bench/file{}.txt", i % 10);
            let _ = fs::metadata(&path);
        } else {
            // 30% write operation: mkdir then rmdir
            let dir_path = format!("/bench/mixed/t{}_{}", thread_id, i);
            let _ = fs::create_dir(&dir_path);
            let _ = fs::remove_dir(&dir_path);
        }
    }
}

/// Scenario 3: Concurrent read from shared file
fn run_read_benchmark(_thread_id: usize, ops: usize) {
    let path = "/bench/read/shared.txt";

    // Open file once
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open {}: {}", path, e);
            return;
        }
    };

    let mut buf = [0u8; 64];
    for _ in 0..ops {
        let _ = file.seek(SeekFrom::Start(0));
        let _ = file.read(&mut buf);
    }
}

/// Scenario 4: File write operations (each thread writes to its own file)
fn run_write_benchmark(thread_id: usize, ops: usize) {
    let path = format!("/bench/io/thread{}.txt", thread_id);

    for i in 0..ops {
        let content = format!("Data from thread {} iteration {}", thread_id, i);

        // Open, write, close for each operation
        if let Ok(mut file) = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
        {
            let _ = file.write_all(content.as_bytes());
        }
    }
}
