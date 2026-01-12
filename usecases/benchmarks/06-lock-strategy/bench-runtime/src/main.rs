//! Benchmark Runtime for Lock Strategy Comparison
//!
//! This benchmark compares three locking strategies:
//! - lock-fine: DashMap + per-inode RwLock (production implementation via vfs-host)
//! - lock-global: HashMap + single RwLock (via vfs-host-global)
//! - lock-none: HashMap + UnsafeCell (via vfs-host-unsafe)
//!
//! The strategy is selected at compile time via feature flags.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::sync::Command;

// Import appropriate VFS host based on feature
#[cfg(feature = "lock-fine")]
use vfs_host::{add_to_linker_with_vfs, Fs, VfsHostState};

#[cfg(feature = "lock-global")]
use vfs_host_global::{add_to_linker_with_vfs, Fs, VfsHostState};

#[cfg(feature = "lock-none")]
use vfs_host_unsafe::{add_to_linker_with_vfs, Fs, VfsHostState};

const THREAD_COUNTS: [usize; 4] = [1, 4, 8, 16];
const OPS_PER_THREAD: usize = 500;

// Data sizes to benchmark (in bytes)
// 1KB (default), 64KB, 1MB
const DATA_SIZES: [usize; 3] = [1024, 64 * 1024, 1024 * 1024];

#[derive(Debug, Clone, Copy)]
struct BenchConfig {
    scenario: &'static str,
    file_scope: &'static str,
    description: &'static str,
}

const SCENARIOS: [BenchConfig; 5] = [
    BenchConfig {
        scenario: "read",
        file_scope: "same",
        description: "Same file parallel reads",
    },
    BenchConfig {
        scenario: "read",
        file_scope: "different",
        description: "Different files parallel reads",
    },
    BenchConfig {
        scenario: "write",
        file_scope: "same",
        description: "Same file parallel writes",
    },
    BenchConfig {
        scenario: "write",
        file_scope: "different",
        description: "Different files parallel writes",
    },
    BenchConfig {
        scenario: "mixed",
        file_scope: "same",
        description: "Mixed read/write same file",
    },
];

/// Get strategy name from compile-time feature
fn strategy_name() -> &'static str {
    #[cfg(feature = "lock-fine")]
    return "lock-fine";

    #[cfg(feature = "lock-global")]
    return "lock-global";

    #[cfg(feature = "lock-none")]
    return "lock-none";
}

fn main() -> Result<()> {
    println!("=== VFS Locking Strategy Benchmark ===");
    println!();
    println!("Strategy: {} (compile-time selected)", strategy_name());
    println!();

    // Create wasmtime engine
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);

    // Load WASM component
    let wasm_path = "../bench-wasm-app/target/wasm32-wasip2/release/bench-lock-wasm-app.wasm";
    let component = Arc::new(
        Component::from_file(&engine, wasm_path).context("Failed to load bench WASM app")?,
    );

    // Print CSV header
    println!("strategy,scenario,file_scope,threads,data_size,total_ops,duration_ms,throughput_ops_sec,errors");

    for data_size in DATA_SIZES {
        println!();
        println!("--- Data size: {} bytes ---", data_size);
        for thread_count in THREAD_COUNTS {
            for config in &SCENARIOS {
                let result = run_benchmark(&engine, &component, config, thread_count, data_size)?;

                println!(
                    "{},{},{},{},{},{},{:.2},{:.0},{}",
                    strategy_name(),
                    config.scenario,
                    config.file_scope,
                    thread_count,
                    data_size,
                    result.total_ops,
                    result.duration_ms,
                    result.throughput,
                    result.error_count,
                );
            }
        }
    }

    println!();
    println!("=== Correctness Verification ===");
    verify_correctness(&engine, &component)?;

    Ok(())
}

#[derive(Debug)]
struct BenchResult {
    total_ops: usize,
    duration_ms: f64,
    throughput: f64,
    error_count: usize,
}

fn run_benchmark(
    engine: &Arc<Engine>,
    component: &Arc<Component>,
    config: &BenchConfig,
    thread_count: usize,
    data_size: usize,
) -> Result<BenchResult> {
    // Create fresh VFS for each benchmark
    let shared_vfs = Arc::new(Fs::new());

    setup_test_data(&shared_vfs, thread_count, data_size)?;

    let start = Instant::now();
    let error_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    for thread_id in 0..thread_count {
        let engine = Arc::clone(engine);
        let component = Arc::clone(component);
        let shared_vfs = Arc::clone(&shared_vfs);
        let scenario = config.scenario.to_string();
        let file_scope = config.file_scope.to_string();
        let errors = Arc::clone(&error_count);

        handles.push(thread::spawn(move || {
            run_wasm_instance(
                &engine,
                &component,
                shared_vfs,
                thread_id,
                OPS_PER_THREAD,
                &scenario,
                &file_scope,
                data_size,
                errors,
            )
        }));
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let duration = start.elapsed();
    let total_ops = thread_count * OPS_PER_THREAD;
    let throughput = total_ops as f64 / duration.as_secs_f64();

    Ok(BenchResult {
        total_ops,
        duration_ms: duration.as_secs_f64() * 1000.0,
        throughput,
        error_count: error_count.load(Ordering::Relaxed),
    })
}

fn run_wasm_instance(
    engine: &Engine,
    component: &Component,
    shared_vfs: Arc<Fs>,
    thread_id: usize,
    ops: usize,
    scenario: &str,
    file_scope: &str,
    data_size: usize,
    error_count: Arc<AtomicUsize>,
) {
    let env_vars = [
        ("BENCH_THREAD_ID", thread_id.to_string()),
        ("BENCH_OPS", ops.to_string()),
        ("BENCH_SCENARIO", scenario.to_string()),
        ("BENCH_FILE_SCOPE", file_scope.to_string()),
        ("BENCH_DATA_SIZE", data_size.to_string()),
    ];
    let env_refs: Vec<(&str, &str)> = env_vars.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let vfs_host_state = VfsHostState::from_shared_vfs_with_env(shared_vfs, &env_refs);

    let mut store = Store::new(engine, vfs_host_state);
    let mut linker = wasmtime::component::Linker::new(engine);

    if let Err(e) = add_to_linker_with_vfs(&mut linker) {
        eprintln!("Failed to add VFS to linker: {}", e);
        error_count.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let command = match Command::instantiate(&mut store, component, &linker) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("Failed to instantiate WASM: {}", e);
            error_count.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    if let Err(_) = command.wasi_cli_run().call_run(&mut store) {
        error_count.fetch_add(1, Ordering::Relaxed);
    }
}

fn setup_test_data(fs: &Arc<Fs>, thread_count: usize, data_size: usize) -> Result<()> {
    // Create directories
    fs.mkdir("/bench")
        .map_err(|e| anyhow::anyhow!("mkdir /bench: {:?}", e))?;
    fs.mkdir("/bench/shared")
        .map_err(|e| anyhow::anyhow!("mkdir /bench/shared: {:?}", e))?;
    fs.mkdir("/bench/files")
        .map_err(|e| anyhow::anyhow!("mkdir /bench/files: {:?}", e))?;

    // Create shared files with initial content (at least data_size bytes)
    let content = vec![b'D'; data_size.max(4096)];
    for name in ["data.txt", "write_target.txt", "mixed.txt"] {
        let path = format!("/bench/shared/{}", name);
        let fd = fs
            .open_path(&path)
            .map_err(|e| anyhow::anyhow!("open {}: {:?}", path, e))?;
        fs.write(fd, &content)
            .map_err(|e| anyhow::anyhow!("write {}: {:?}", path, e))?;
        fs.close(fd)
            .map_err(|e| anyhow::anyhow!("close {}: {:?}", path, e))?;
    }

    // Create per-thread files with data_size bytes
    let thread_content = vec![b'T'; data_size.max(1024)];
    for i in 0..thread_count {
        let path = format!("/bench/files/thread_{}.txt", i);
        let fd = fs
            .open_path(&path)
            .map_err(|e| anyhow::anyhow!("open {}: {:?}", path, e))?;
        fs.write(fd, &thread_content)
            .map_err(|e| anyhow::anyhow!("write {}: {:?}", path, e))?;
        fs.close(fd)
            .map_err(|e| anyhow::anyhow!("close {}: {:?}", path, e))?;
    }

    Ok(())
}

/// Verify data integrity after multi-threaded operations
fn verify_correctness(
    _engine: &Arc<Engine>,
    _component: &Arc<Component>,
) -> Result<()> {
    let shared_vfs = Arc::new(Fs::new());

    // Setup
    shared_vfs
        .mkdir("/verify")
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;

    // Create verification file
    let fd = shared_vfs
        .open_path("/verify/append_test.txt")
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;
    shared_vfs
        .close(fd)
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;

    let num_threads = 8;
    let appends_per_thread = 50;
    let error_count = Arc::new(AtomicUsize::new(0));

    // Each thread appends a unique marker
    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let vfs = Arc::clone(&shared_vfs);
        let errors = Arc::clone(&error_count);

        handles.push(thread::spawn(move || {
            for i in 0..appends_per_thread {
                let fd = match vfs.open_path("/verify/append_test.txt") {
                    Ok(fd) => fd,
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };

                // Seek to end and write
                if let Err(_) = vfs.seek(fd, 0, 2) {
                    // SEEK_END = 2
                    errors.fetch_add(1, Ordering::Relaxed);
                    let _ = vfs.close(fd);
                    continue;
                }

                let marker = format!("T{}I{}\n", thread_id, i);
                if let Err(_) = vfs.write(fd, marker.as_bytes()) {
                    errors.fetch_add(1, Ordering::Relaxed);
                }

                let _ = vfs.close(fd);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Read back and verify
    let fd = shared_vfs
        .open_path("/verify/append_test.txt")
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;
    let mut content = vec![0u8; 1024 * 1024];
    let bytes_read = shared_vfs
        .read(fd, &mut content)
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;
    shared_vfs
        .close(fd)
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;

    let content = String::from_utf8_lossy(&content[..bytes_read]);
    let line_count = content.lines().count();
    let expected_lines = num_threads * appends_per_thread;
    let errors = error_count.load(Ordering::Relaxed);

    if line_count == expected_lines && errors == 0 {
        println!(
            "PASS: All {} appends recorded correctly (strategy: {})",
            expected_lines,
            strategy_name()
        );
    } else {
        println!(
            "WARN: Expected {} lines, found {} (errors: {}, strategy: {})",
            expected_lines,
            line_count,
            errors,
            strategy_name()
        );
        #[cfg(feature = "lock-none")]
        println!("      (Data loss expected with lock-none due to data races)");
    }

    Ok(())
}
