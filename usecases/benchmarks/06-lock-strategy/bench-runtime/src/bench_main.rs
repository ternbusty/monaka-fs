//! Benchmark for main-branch fs-core (Rc<RefCell>, no internal locking)
//! Direct fs-core calls without WASM (main-branch fs-core is !Send, can't use with wasmtime)
//!
//! This measures the raw fs-core performance without wasmtime overhead,
//! providing a baseline for comparing locking strategies.

mod common;

use anyhow::Result;
use common::{print_csv_header, BenchConfig, BenchResult, BenchTimer, DATA_SIZES, OPS_PER_THREAD, SCENARIOS};
use fs_main::Fs;

const STRATEGY_NAME: &str = "main-direct";

fn main() -> Result<()> {
    eprintln!("=== VFS Locking Strategy Benchmark ===");
    eprintln!();
    eprintln!(
        "Strategy: {} (main-branch fs-core, direct calls without WASM)",
        STRATEGY_NAME
    );
    eprintln!("Note: This is !Send, so no wasmtime integration possible");
    eprintln!();

    print_csv_header();

    let thread_count = 1;

    for data_size in DATA_SIZES {
        eprintln!();
        eprintln!("--- Data size: {} bytes ---", data_size);

        for scenario in &SCENARIOS {
            let result = run_benchmark(scenario, data_size)?;

            let integrity = "N/A".to_string();

            println!(
                "{},{},{},{},{},{},{:.2},{:.0},{},{}",
                STRATEGY_NAME,
                scenario.scenario,
                scenario.file_scope,
                thread_count,
                data_size,
                result.total_ops,
                result.duration_ms,
                result.throughput,
                result.error_count,
                integrity,
            );
        }
    }

    Ok(())
}

fn run_benchmark(config: &BenchConfig, data_size: usize) -> Result<BenchResult> {
    let mut fs = Fs::new();

    // Setup test data
    setup_test_data(&mut fs, data_size)?;

    let timer = BenchTimer::new();
    let mut error_count = 0;

    // Run operations
    match (config.scenario, config.file_scope) {
        ("read", "same") => {
            let fd = fs.open_path("/bench/shared/data.txt")?;
            let mut buf = vec![0u8; data_size];
            for _ in 0..OPS_PER_THREAD {
                fs.seek(fd, 0, 0)?; // SEEK_SET
                if fs.read(fd, &mut buf).is_err() {
                    error_count += 1;
                }
            }
            fs.close(fd)?;
        }
        ("read", "different") => {
            let fd = fs.open_path("/bench/files/thread_0.txt")?;
            let mut buf = vec![0u8; data_size];
            for _ in 0..OPS_PER_THREAD {
                fs.seek(fd, 0, 0)?;
                if fs.read(fd, &mut buf).is_err() {
                    error_count += 1;
                }
            }
            fs.close(fd)?;
        }
        ("write", "same") => {
            let fd = fs.open_path("/bench/shared/write_target.txt")?;
            let data = format!("T0:{}\n", "X".repeat(data_size.saturating_sub(5)));
            for _ in 0..OPS_PER_THREAD {
                if fs.append_write(fd, data.as_bytes()).is_err() {
                    error_count += 1;
                }
            }
            fs.close(fd)?;
        }
        ("write", "different") => {
            let fd = fs.open_path("/bench/files/thread_0.txt")?;
            let data = vec![b'W'; data_size];
            for _ in 0..OPS_PER_THREAD {
                fs.seek(fd, 0, 0)?;
                if fs.write(fd, &data).is_err() {
                    error_count += 1;
                }
            }
            fs.close(fd)?;
        }
        ("mixed", _) => {
            let fd = fs.open_path("/bench/shared/mixed.txt")?;
            let mut buf = vec![0u8; data_size];
            let write_data = vec![b'M'; data_size];
            for i in 0..OPS_PER_THREAD {
                fs.seek(fd, 0, 0)?;
                if i % 2 == 0 {
                    if fs.read(fd, &mut buf).is_err() {
                        error_count += 1;
                    }
                } else {
                    if fs.write(fd, &write_data).is_err() {
                        error_count += 1;
                    }
                }
            }
            fs.close(fd)?;
        }
        _ => {}
    }

    let total_ops = OPS_PER_THREAD;

    Ok(BenchResult {
        total_ops,
        duration_ms: timer.elapsed_ms(),
        throughput: timer.throughput(total_ops),
        error_count,
    })
}

fn setup_test_data(fs: &mut Fs, data_size: usize) -> Result<()> {
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
    let path = "/bench/files/thread_0.txt".to_string();
    let fd = fs.open_path(&path)?;
    fs.write(fd, &thread_content)?;
    fs.close(fd)?;

    Ok(())
}
