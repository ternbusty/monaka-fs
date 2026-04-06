//! Benchmark: tmpfs vs VFS
//!
//! This benchmark measures sequential write, sequential read, and random read
//! performance for various file sizes.
//!
//! Modes:
//! - Default: Full benchmark (write + read) - for Halycon VFS
//! - --seq-read-only: Sequential read of pre-created files only
//! - --random-read-only: Random read of pre-created files only

use std::env;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Instant;

const ITERATIONS: usize = 5;
const RANDOM_READ_BLOCK_SIZE: usize = 4096;
const RANDOM_READ_COUNT: usize = 1000;

fn main() {
    let args: Vec<String> = env::args().collect();
    let seq_read_only = args.iter().any(|a| a == "--seq-read-only");
    let random_read_only = args.iter().any(|a| a == "--random-read-only");

    // Ensure /mnt directory exists (for host tmpfs mode)
    let _ = fs::create_dir("/mnt");

    let file_sizes: &[(usize, &str)] = &[
        (1 * 1024 * 1024, "1MB"),
        (10 * 1024 * 1024, "10MB"),
        (100 * 1024 * 1024, "100MB"),
    ];

    if seq_read_only {
        for &(size, label) in file_sizes {
            run_seq_read_only(size, label);
        }
    } else if random_read_only {
        for &(size, label) in file_sizes {
            run_random_read_only(size, label);
        }
    } else {
        for &(size, label) in file_sizes {
            run_full_benchmark(size, label);
        }
    }
}

/// Full benchmark: write + read (for Halycon VFS where no host cache exists)
fn run_full_benchmark(file_size: usize, label: &str) {
    let data = generate_test_data(file_size);

    // Sequential Write - use different file for each iteration
    let write_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/mnt/benchmark_write_{}.dat", i);
            let start = Instant::now();
            let mut file = File::create(&path).expect("Failed to create file");
            file.write_all(&data).expect("Failed to write file");
            file.sync_all().expect("Failed to sync file");
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            drop(file);
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

    // Sequential Read - create files first, then read each once
    for i in 0..ITERATIONS {
        let path = format!("/mnt/benchmark_read_{}.dat", i);
        let mut file = File::create(&path).expect("Failed to create file for read test");
        file.write_all(&data).expect("Failed to write file for read test");
        file.sync_all().expect("Failed to sync file for read test");
    }
    let read_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/mnt/benchmark_read_{}.dat", i);
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

    // Random Read
    if file_size >= RANDOM_READ_BLOCK_SIZE {
        for i in 0..ITERATIONS {
            let path = format!("/mnt/benchmark_random_{}.dat", i);
            let mut file = File::create(&path).expect("Failed to create file for random read test");
            file.write_all(&data).expect("Failed to write file for random read test");
            file.sync_all().expect("Failed to sync file for random read test");
        }
        let random_durations: Vec<f64> = (0..ITERATIONS)
            .map(|iter| {
                let path = format!("/mnt/benchmark_random_{}.dat", iter);
                let mut file = File::open(&path).expect("Failed to open file");
                let max_offset = file_size - RANDOM_READ_BLOCK_SIZE;
                let mut rng = SimpleRng::new(12345 + iter as u64);
                let mut buf = vec![0u8; RANDOM_READ_BLOCK_SIZE];

                let start = Instant::now();
                for _ in 0..RANDOM_READ_COUNT {
                    let offset = rng.next_range(max_offset as u64) as u64;
                    file.seek(SeekFrom::Start(offset)).expect("Failed to seek");
                    file.read_exact(&mut buf).expect("Failed to read");
                }
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                drop(file);
                let _ = fs::remove_file(&path);
                elapsed
            })
            .collect();
        let random_ms = median(&random_durations);
        let total_bytes = RANDOM_READ_COUNT * RANDOM_READ_BLOCK_SIZE;
        let random_throughput = (total_bytes as f64 / (1024.0 * 1024.0)) / (random_ms / 1000.0);
        println!(
            "[RESULT] random_read,{},{:.3},{:.2}",
            label, random_ms, random_throughput
        );
    }
}

/// Sequential read-only: read pre-created files (for host FS with cleared cache)
/// Expects files like /mnt/1mb_0.dat, /mnt/1mb_1.dat, ...
fn run_seq_read_only(file_size: usize, label: &str) {
    let filename = label.to_lowercase();
    let read_durations: Vec<f64> = (0..ITERATIONS)
        .map(|i| {
            let path = format!("/mnt/{}_{}.dat", filename, i);
            let start = Instant::now();
            let _ = fs::read(&path).expect(&format!("Failed to read file: {}", path));
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    let read_ms = median(&read_durations);
    let read_throughput = (file_size as f64 / (1024.0 * 1024.0)) / (read_ms / 1000.0);
    println!(
        "[RESULT] seq_read,{},{:.3},{:.2}",
        label, read_ms, read_throughput
    );
}

/// Random read-only: read pre-created files at random offsets (for host FS with cleared cache)
/// Expects files like /mnt/1mb_0.dat, /mnt/1mb_1.dat, ...
fn run_random_read_only(file_size: usize, label: &str) {
    if file_size < RANDOM_READ_BLOCK_SIZE {
        return;
    }
    let filename = label.to_lowercase();
    let random_durations: Vec<f64> = (0..ITERATIONS)
        .map(|iter| {
            let path = format!("/mnt/{}_{}.dat", filename, iter);
            let mut file = File::open(&path).expect("Failed to open file");
            let max_offset = file_size - RANDOM_READ_BLOCK_SIZE;
            let mut rng = SimpleRng::new(12345 + iter as u64);
            let mut buf = vec![0u8; RANDOM_READ_BLOCK_SIZE];

            let start = Instant::now();
            for _ in 0..RANDOM_READ_COUNT {
                let offset = rng.next_range(max_offset as u64) as u64;
                file.seek(SeekFrom::Start(offset)).expect("Failed to seek");
                file.read_exact(&mut buf).expect("Failed to read");
            }
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    let random_ms = median(&random_durations);
    let total_bytes = RANDOM_READ_COUNT * RANDOM_READ_BLOCK_SIZE;
    let random_throughput = (total_bytes as f64 / (1024.0 * 1024.0)) / (random_ms / 1000.0);
    println!(
        "[RESULT] random_read,{},{:.3},{:.2}",
        label, random_ms, random_throughput
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

/// Simple pseudo-random number generator (deterministic for reproducibility)
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn next_range(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        self.next_u64() % max
    }
}
