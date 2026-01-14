//! WASM Benchmark App for Lock Strategy Comparison
//!
//! This app performs file operations based on environment variables:
//! - BENCH_THREAD_ID: Thread/instance identifier
//! - BENCH_OPS: Number of operations to perform
//! - BENCH_SCENARIO: read, write, or mixed (or "setup" for initialization)
//! - BENCH_FILE_SCOPE: same (shared file) or different (per-thread file)
//! - BENCH_DATA_SIZE: Data size in bytes for read/write operations (default: 1024)
//! - BENCH_THREAD_COUNT: Number of threads (for setup only, default: 8)

use std::fs::{create_dir_all, File};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Instant;

fn main() {
    let thread_id: usize = env("BENCH_THREAD_ID").parse().unwrap_or(0);
    let ops: usize = env("BENCH_OPS").parse().unwrap_or(100);
    let scenario = env("BENCH_SCENARIO");
    let file_scope = env("BENCH_FILE_SCOPE");
    let data_size: usize = env("BENCH_DATA_SIZE").parse().unwrap_or(1024);
    let thread_count: usize = env("BENCH_THREAD_COUNT").parse().unwrap_or(8);

    let start = Instant::now();
    let result = match (scenario.as_str(), file_scope.as_str()) {
        ("setup", _) => setup_bench_files(thread_count, data_size),
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
    let elapsed = start.elapsed();

    // Output result in machine-parseable format
    let errors = match &result {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    };

    println!(
        "BENCH_RESULT:thread={},scenario={}_{},ops={},elapsed_us={},errors={}",
        thread_id,
        scenario,
        file_scope,
        ops,
        elapsed.as_micros(),
        errors
    );
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Same file parallel reads - tests read lock contention
fn bench_read_same_file(ops: usize, data_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    let path = "/bench/shared/data.txt";
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; data_size];

    for _ in 0..ops {
        file.seek(SeekFrom::Start(0))?;
        let _ = file.read(&mut buf)?;
    }
    Ok(())
}

/// Different files parallel reads - tests DashMap scalability
fn bench_read_different_files(thread_id: usize, ops: usize, data_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("/bench/files/thread_{}.txt", thread_id);
    let mut file = File::open(&path)?;
    let mut buf = vec![0u8; data_size];

    for _ in 0..ops {
        file.seek(SeekFrom::Start(0))?;
        let _ = file.read(&mut buf)?;
    }
    Ok(())
}

/// Same file parallel writes - tests write lock contention
fn bench_write_same_file(thread_id: usize, ops: usize, data_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    let path = "/bench/shared/write_target.txt";
    // Create write data of specified size
    let data = format!("T{}:", thread_id)
        .chars()
        .chain(std::iter::repeat('X').take(data_size.saturating_sub(10)))
        .chain("\n".chars())
        .collect::<String>();

    for _ in 0..ops {
        // Open with O_APPEND for atomic appends
        let mut file = OpenOptions::new().append(true).open(path)?;
        file.write_all(data.as_bytes())?;
    }
    Ok(())
}

/// Different files parallel writes - tests independent inode access
fn bench_write_different_files(thread_id: usize, ops: usize, data_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("/bench/files/thread_{}.txt", thread_id);
    // Create write data of specified size
    let data = vec![b'A'; data_size];

    for _ in 0..ops {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)?;
        file.write_all(&data)?;
    }
    Ok(())
}

/// Mixed read/write on same file - tests read/write lock interaction
fn bench_mixed_same_file(_thread_id: usize, ops: usize, data_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    let path = "/bench/shared/mixed.txt";
    let mut buf = vec![0u8; data_size];
    let write_data = vec![b'W'; data_size];

    for i in 0..ops {
        if i % 5 == 0 {
            // 20% writes
            let mut file = OpenOptions::new().append(true).open(path)?;
            file.write_all(&write_data)?;
        } else {
            // 80% reads
            let mut file = File::open(path)?;
            let _ = file.read(&mut buf)?;
        }
    }
    Ok(())
}

/// Setup benchmark files - creates directories and initial test files
fn setup_bench_files(thread_count: usize, data_size: usize) -> Result<(), Box<dyn std::error::Error>> {
    // Create directories
    create_dir_all("/bench/shared")?;
    create_dir_all("/bench/files")?;

    // Create shared files with initial content
    let content = vec![b'D'; data_size.max(4096)];
    for name in ["data.txt", "write_target.txt", "mixed.txt"] {
        let path = format!("/bench/shared/{}", name);
        let mut file = File::create(&path)?;
        file.write_all(&content)?;
    }

    // Create per-thread files
    let thread_content = vec![b'T'; data_size.max(1024)];
    for i in 0..thread_count {
        let path = format!("/bench/files/thread_{}.txt", i);
        let mut file = File::create(&path)?;
        file.write_all(&thread_content)?;
    }

    eprintln!("Setup complete: created {} thread files", thread_count);
    Ok(())
}
