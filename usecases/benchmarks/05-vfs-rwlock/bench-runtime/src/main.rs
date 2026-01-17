//! VFS Concurrent WASM Benchmark Runtime
//!
//! Runs multiple WASM instances in parallel, each accessing the same shared VFS.
//! Measures throughput and latency of concurrent file operations.

use anyhow::{Context, Result};
use fs_core::Fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::sync::Command;

const NUM_THREADS: usize = 8;
const OPS_PER_THREAD: usize = 1000;

// fs-core open flags
const O_WRONLY: u32 = 1;
const O_CREAT: u32 = 0o100;
const O_TRUNC: u32 = 0o1000;

fn main() -> Result<()> {
    println!("=== VFS Concurrent WASM Benchmark ===");
    println!("Threads: {}, Ops per thread: {}", NUM_THREADS, OPS_PER_THREAD);
    println!();

    // Create wasmtime engine (shared across all threads)
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);

    // Load WASM component (shared across all threads)
    let wasm_path = "../bench-wasm-app/target/wasm32-wasip2/release/bench-wasm-app.wasm";
    let component = Arc::new(
        Component::from_file(&engine, wasm_path)
            .context("Failed to load bench-wasm-app.wasm")?
    );

    // Create shared VFS
    let shared_vfs = Arc::new(Fs::new());

    // Setup test data
    setup_test_data(&shared_vfs)?;

    println!("Running benchmarks with {} parallel WASM instances...", NUM_THREADS);
    println!();

    // Benchmark 1: Concurrent stat() operations
    println!("Scenario 1: Concurrent stat() (read-only)");
    let stat_result = bench_scenario(&engine, &component, &shared_vfs, "stat", OPS_PER_THREAD)?;
    print_results(&stat_result);

    // Recreate shared VFS for next benchmark (clean state)
    let shared_vfs = Arc::new(Fs::new());
    setup_test_data(&shared_vfs)?;

    // Benchmark 2: Mixed workload
    println!("Scenario 2: Mixed workload (70% stat, 30% mkdir/rmdir)");
    let mixed_result = bench_scenario(&engine, &component, &shared_vfs, "mixed", OPS_PER_THREAD)?;
    print_results(&mixed_result);

    // Benchmark 3: Concurrent read
    let shared_vfs = Arc::new(Fs::new());
    setup_test_data(&shared_vfs)?;

    println!("Scenario 3: Concurrent read - SAME file");
    let read_result = bench_scenario(&engine, &component, &shared_vfs, "read", OPS_PER_THREAD)?;
    print_results(&read_result);

    // Benchmark 4: File write
    let shared_vfs = Arc::new(Fs::new());
    setup_test_data(&shared_vfs)?;

    println!("Scenario 4: File write operations");
    let write_result = bench_scenario(&engine, &component, &shared_vfs, "write", OPS_PER_THREAD / 10)?;
    print_results(&write_result);

    // Summary
    println!("========================================");
    println!("SUMMARY");
    println!("========================================");
    println!("{:<25} {:>15}", "Scenario", "Throughput");
    println!("{:-<25} {:-<15}", "", "");
    println!("{:<25} {:>12.0} ops/sec", "stat() read-only", stat_result.throughput());
    println!("{:<25} {:>12.0} ops/sec", "Mixed workload", mixed_result.throughput());
    println!("{:<25} {:>12.0} ops/sec", "Concurrent read", read_result.throughput());
    println!("{:<25} {:>12.0} ops/sec", "File write", write_result.throughput());

    Ok(())
}

struct BenchResult {
    total_ops: usize,
    duration: Duration,
    thread_durations: Vec<Duration>,
}

impl BenchResult {
    fn throughput(&self) -> f64 {
        self.total_ops as f64 / self.duration.as_secs_f64()
    }

    fn avg_thread_duration(&self) -> Duration {
        let total: Duration = self.thread_durations.iter().sum();
        total / self.thread_durations.len() as u32
    }
}

fn print_results(result: &BenchResult) {
    println!("  Total ops: {}", result.total_ops);
    println!("  Duration: {:?}", result.duration);
    println!("  Throughput: {:.0} ops/sec", result.throughput());
    println!("  Avg thread duration: {:?}", result.avg_thread_duration());
    println!();
}

fn setup_test_data(fs: &Arc<Fs>) -> Result<()> {
    // Create test directories
    fs.mkdir("/bench").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/mixed").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/read").map_err(|e| anyhow::anyhow!("{:?}", e))?;
    fs.mkdir("/bench/io").map_err(|e| anyhow::anyhow!("{:?}", e))?;

    // Create test files for stat benchmark
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

    // Create shared file for read benchmark
    {
        let path = "/bench/read/shared.txt";
        let fd = fs
            .open_path_with_flags(path, O_WRONLY | O_CREAT | O_TRUNC)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let content = "Test content for shared file - all threads read this same data!";
        fs.write(fd, content.as_bytes())
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }

    // Create files for each thread (write benchmark)
    for i in 0..NUM_THREADS {
        let path = format!("/bench/io/thread{}.txt", i);
        let fd = fs
            .open_path_with_flags(&path, O_WRONLY | O_CREAT | O_TRUNC)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        fs.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }

    Ok(())
}

fn bench_scenario(
    engine: &Arc<Engine>,
    component: &Arc<Component>,
    shared_vfs: &Arc<Fs>,
    scenario: &str,
    ops: usize,
) -> Result<BenchResult> {
    let start = Instant::now();
    let mut handles = vec![];
    let thread_durations = Arc::new(std::sync::Mutex::new(Vec::new()));

    for thread_id in 0..NUM_THREADS {
        let engine = Arc::clone(engine);
        let component = Arc::clone(component);
        let shared_vfs = Arc::clone(shared_vfs);
        let scenario = scenario.to_string();
        let durations = Arc::clone(&thread_durations);

        handles.push(thread::spawn(move || {
            let thread_start = Instant::now();

            // Create VfsHostState that shares the VFS
            let env_vars = [
                ("BENCH_THREAD_ID", thread_id.to_string()),
                ("BENCH_OPS", ops.to_string()),
                ("BENCH_SCENARIO", scenario),
            ];
            let env_refs: Vec<(&str, &str)> = env_vars
                .iter()
                .map(|(k, v)| (*k, v.as_str()))
                .collect();

            let vfs_host_state = vfs_host::VfsHostState::from_shared_vfs_with_env(
                shared_vfs,
                &env_refs,
            );

            // Create store and linker for this thread
            let mut store = Store::new(&engine, vfs_host_state);
            let mut linker = wasmtime::component::Linker::new(&engine);
            vfs_host::add_to_linker_with_vfs(&mut linker).unwrap();

            // Instantiate and run WASM
            let command = Command::instantiate(&mut store, &component, &linker)
                .expect("Failed to instantiate WASM");

            match command.wasi_cli_run().call_run(&mut store) {
                Ok(Ok(())) => {}
                Ok(Err(())) => eprintln!("Thread {} WASM returned error", thread_id),
                Err(e) => eprintln!("Thread {} WASM execution failed: {:?}", thread_id, e),
            }

            durations.lock().unwrap().push(thread_start.elapsed());
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start.elapsed();
    let thread_durations = Arc::try_unwrap(thread_durations)
        .unwrap()
        .into_inner()
        .unwrap();

    Ok(BenchResult {
        total_ops: NUM_THREADS * ops,
        duration,
        thread_durations,
    })
}
