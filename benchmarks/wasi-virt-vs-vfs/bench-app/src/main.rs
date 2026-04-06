//! Benchmark: wasi-virt vs VFS (Read-only)
//!
//! This benchmark measures read performance for pre-embedded files.
//! Both wasi-virt and Monaka support pre-embedding files into WASM binaries.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::Instant;

const ITERATIONS: usize = 5;
const RANDOM_READ_BLOCK_SIZE: usize = 4096;
const RANDOM_READ_COUNT: usize = 1000;

fn main() {
    println!("=== Benchmark: wasi-virt vs VFS (Read-only) ===");
    println!("Reading pre-embedded files from /data");
    println!();

    let file_sizes: &[(&str, &str)] = &[
        ("/data/1mb.dat", "1MB"),
        ("/data/10mb.dat", "10MB"),
        ("/data/100mb.dat", "100MB"),
    ];

    for &(path, label) in file_sizes {
        // Check if file exists
        match std::fs::metadata(path) {
            Ok(meta) => {
                println!("--- File Size: {} (actual: {} bytes) ---", label, meta.len());
                run_benchmark(path, label, meta.len() as usize);
                println!();
            }
            Err(e) => {
                println!("--- File Size: {} ---", label);
                println!("[SKIP] File not found: {} ({})", path, e);
                println!();
            }
        }
    }

    println!("=== Benchmark Complete ===");
}

fn run_benchmark(path: &str, label: &str, file_size: usize) {
    // Sequential Read
    let read_durations: Vec<f64> = (0..ITERATIONS)
        .map(|_| {
            let start = Instant::now();
            let _ = std::fs::read(path).expect("Failed to read file");
            start.elapsed().as_secs_f64() * 1000.0
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
        let random_durations: Vec<f64> = (0..ITERATIONS)
            .map(|iter| {
                let mut file = File::open(path).expect("Failed to open file");
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
