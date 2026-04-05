//! WASM Benchmark App for Lock Strategy Comparison
//!
//! This app performs file operations based on environment variables:
//! - BENCH_THREAD_ID: Thread/instance identifier
//! - BENCH_OPS: Number of operations to perform
//! - BENCH_SCENARIO: read, write, or mixed
//! - BENCH_FILE_SCOPE: same (shared file) or different (per-thread file)
//! - BENCH_DATA_SIZE: Data size in bytes for read/write operations (default: 1024)
//! - BENCH_NO_CACHE: Set to "1" to enable cache pollution (flush cache between ops)
//!
//! Note: Test data setup is done by the host (bench_fine.rs) before WASM instances start.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

/// Cache pollution buffer size (32MB - larger than typical L3 cache)
const POLLUTION_BUF_SIZE: usize = 32 * 1024 * 1024;

/// Global flag for cache pollution mode
static mut NO_CACHE_MODE: bool = false;

/// Pollution buffer - allocated lazily
static mut POLLUTION_BUF: Option<Vec<u8>> = None;

/// Pollute CPU cache by reading a large buffer
/// This evicts other data from L1/L2/L3 caches
#[inline(never)]
fn pollute_cache() {
    unsafe {
        if !NO_CACHE_MODE {
            return;
        }

        // Lazy initialization of pollution buffer
        if POLLUTION_BUF.is_none() {
            // Use varying pattern to prevent optimization
            let mut buf = vec![0u8; POLLUTION_BUF_SIZE];
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (i & 0xFF) as u8;
            }
            POLLUTION_BUF = Some(buf);
        }

        if let Some(ref buf) = POLLUTION_BUF {
            // Read entire buffer to pollute cache
            // Use volatile-like access pattern to prevent optimization
            let mut sum: u64 = 0;
            for chunk in buf.chunks(64) {
                // Access each cache line (64 bytes)
                sum = sum.wrapping_add(chunk[0] as u64);
            }
            // Prevent dead code elimination
            std::hint::black_box(sum);
        }
    }
}

fn main() {
    let thread_id: usize = env("BENCH_THREAD_ID").parse().unwrap_or(0);
    let ops: usize = env("BENCH_OPS").parse().unwrap_or(100);
    let scenario = env("BENCH_SCENARIO");
    let file_scope = env("BENCH_FILE_SCOPE");
    let data_size: usize = env("BENCH_DATA_SIZE").parse().unwrap_or(1024);

    // Enable cache pollution mode if requested
    let no_cache = env("BENCH_NO_CACHE") == "1";
    unsafe {
        NO_CACHE_MODE = no_cache;
    }
    if no_cache {
        eprintln!("[NO_CACHE] Cache pollution enabled (32MB buffer)");
    }

    let result = match (scenario.as_str(), file_scope.as_str()) {
        ("read", "same") => bench_read_same_file(ops, data_size),
        ("read", "different") => bench_read_different_files(thread_id, ops, data_size),
        ("write", "same") => bench_write_same_file(thread_id, ops, data_size),
        ("write", "different") => bench_write_different_files(thread_id, ops, data_size),
        ("mixed", "same") => bench_mixed_same_file(thread_id, ops, data_size),
        _ => {
            eprintln!("Unknown scenario: {} / {}", scenario, file_scope);
            Err("Unknown scenario".into())
        }
    };

    // Output result in machine-parseable format
    let (elapsed_us, errors) = match &result {
        Ok(duration) => (duration.as_micros(), 0),
        Err(e) => {
            eprintln!("Error: {}", e);
            (0, 1)
        }
    };

    println!(
        "BENCH_RESULT:thread={},scenario={}_{},ops={},elapsed_us={},errors={}",
        thread_id,
        scenario,
        file_scope,
        ops,
        elapsed_us,
        errors
    );
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Same file parallel reads - tests read lock contention
fn bench_read_same_file(ops: usize, data_size: usize) -> Result<Duration, Box<dyn std::error::Error>> {
    let path = "/bench/shared/data.txt";
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; data_size];
    let mut total_time = Duration::ZERO;

    for _ in 0..ops {
        pollute_cache(); // Evict file data from CPU cache (not timed)
        let start = Instant::now();
        file.seek(SeekFrom::Start(0))?;
        let _ = file.read(&mut buf)?;
        total_time += start.elapsed();
    }
    Ok(total_time)
}

/// Different files parallel reads - tests DashMap scalability
fn bench_read_different_files(thread_id: usize, ops: usize, data_size: usize) -> Result<Duration, Box<dyn std::error::Error>> {
    let path = format!("/bench/files/thread_{}.txt", thread_id);
    let mut file = File::open(&path)?;
    let mut buf = vec![0u8; data_size];
    let mut total_time = Duration::ZERO;

    for _ in 0..ops {
        pollute_cache(); // Evict file data from CPU cache (not timed)
        let start = Instant::now();
        file.seek(SeekFrom::Start(0))?;
        let _ = file.read(&mut buf)?;
        total_time += start.elapsed();
    }
    Ok(total_time)
}

/// Same file parallel writes - tests write lock contention
fn bench_write_same_file(thread_id: usize, ops: usize, data_size: usize) -> Result<Duration, Box<dyn std::error::Error>> {
    let path = "/bench/shared/write_target.txt";
    // Create write data of specified size
    let data = format!("T{}:", thread_id)
        .chars()
        .chain(std::iter::repeat('X').take(data_size.saturating_sub(10)))
        .chain("\n".chars())
        .collect::<String>();
    let mut total_time = Duration::ZERO;

    for _ in 0..ops {
        pollute_cache(); // Evict file data from CPU cache (not timed)
        let start = Instant::now();
        // Open with O_APPEND for atomic appends
        let mut file = OpenOptions::new().append(true).open(path)?;
        file.write_all(data.as_bytes())?;
        total_time += start.elapsed();
    }
    Ok(total_time)
}

/// Different files parallel writes - tests independent inode access
fn bench_write_different_files(thread_id: usize, ops: usize, data_size: usize) -> Result<Duration, Box<dyn std::error::Error>> {
    let path = format!("/bench/files/thread_{}.txt", thread_id);
    // Create write data of specified size
    let data = vec![b'A'; data_size];
    let mut total_time = Duration::ZERO;

    for _ in 0..ops {
        pollute_cache(); // Evict file data from CPU cache (not timed)
        let start = Instant::now();
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)?;
        file.write_all(&data)?;
        total_time += start.elapsed();
    }
    Ok(total_time)
}

/// Mixed read/write on same file - tests read/write lock interaction
fn bench_mixed_same_file(_thread_id: usize, ops: usize, data_size: usize) -> Result<Duration, Box<dyn std::error::Error>> {
    let path = "/bench/shared/mixed.txt";
    let mut buf = vec![0u8; data_size];
    let write_data = vec![b'W'; data_size];
    let mut total_time = Duration::ZERO;

    for i in 0..ops {
        pollute_cache(); // Evict file data from CPU cache (not timed)
        let start = Instant::now();
        if i % 5 == 0 {
            // 20% writes
            let mut file = OpenOptions::new().append(true).open(path)?;
            file.write_all(&write_data)?;
        } else {
            // 80% reads
            let mut file = File::open(path)?;
            let _ = file.read(&mut buf)?;
        }
        total_time += start.elapsed();
    }
    Ok(total_time)
}


