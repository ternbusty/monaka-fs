//! Benchmark for lock-global strategy (HashMap + single RwLock)

mod common;

use anyhow::{Context, Result};
use common::{
    print_csv_header, setup_test_data, verify_benchmark_results, BenchConfig, BenchResult,
    BenchTimer, VfsOps, DATA_SIZES, OPS_PER_THREAD, SCENARIOS, THREAD_COUNTS,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use vfs_host_global::{add_to_linker_with_vfs, Fs, VfsHostState};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::sync::Command;

const STRATEGY_NAME: &str = "lock-global";

impl VfsOps for Fs {
    fn mkdir(&self, path: &str) -> Result<(), anyhow::Error> {
        self.mkdir(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn open_path(&self, path: &str) -> Result<u32, anyhow::Error> {
        self.open_path(path).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error> {
        self.write(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, anyhow::Error> {
        self.read(fd, buf).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn append_write(&self, fd: u32, buf: &[u8]) -> Result<usize, anyhow::Error> {
        self.append_write(fd, buf)
            .map_err(|e| anyhow::anyhow!("{:?}", e))
    }
    fn close(&self, fd: u32) -> Result<(), anyhow::Error> {
        self.close(fd).map_err(|e| anyhow::anyhow!("{:?}", e))
    }
}

fn main() -> Result<()> {
    eprintln!("=== VFS Locking Strategy Benchmark ===");
    eprintln!();
    eprintln!("Strategy: {} (global locking)", STRATEGY_NAME);
    eprintln!();

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);

    let wasm_path = "../bench-wasm-app/target/wasm32-wasip2/release/bench-lock-wasm-app.wasm";
    let component = Arc::new(
        Component::from_file(&engine, wasm_path).context("Failed to load bench WASM app")?,
    );

    print_csv_header();

    for data_size in DATA_SIZES {
        eprintln!();
        eprintln!("--- Data size: {} bytes ---", data_size);
        for thread_count in THREAD_COUNTS {
            for scenario in &SCENARIOS {
                let (result, shared_vfs) =
                    run_benchmark(&engine, &component, scenario, thread_count, data_size)?;

                // Verify data integrity for write/same scenario
                let integrity = if scenario.scenario == "write" && scenario.file_scope == "same" {
                    let correctness =
                        verify_benchmark_results(&*shared_vfs, thread_count, OPS_PER_THREAD, data_size)?;
                    format!("{:.1}%", correctness.integrity_percent())
                } else {
                    "N/A".to_string()
                };

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
    }

    Ok(())
}

fn run_benchmark(
    engine: &Arc<Engine>,
    component: &Arc<Component>,
    config: &BenchConfig,
    thread_count: usize,
    data_size: usize,
) -> Result<(BenchResult, Arc<Fs>)> {
    let shared_vfs = Arc::new(Fs::new());
    setup_test_data(&*shared_vfs, thread_count, data_size)?;

    let timer = BenchTimer::new();
    let error_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    for thread_id in 0..thread_count {
        let engine = Arc::clone(engine);
        let component = Arc::clone(component);
        let vfs = Arc::clone(&shared_vfs);
        let scenario = config.scenario.to_string();
        let file_scope = config.file_scope.to_string();
        let errors = Arc::clone(&error_count);

        handles.push(thread::spawn(move || {
            run_wasm_instance(
                &engine,
                &component,
                vfs,
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

    let total_ops = thread_count * OPS_PER_THREAD;

    Ok((
        BenchResult {
            total_ops,
            duration_ms: timer.elapsed_ms(),
            throughput: timer.throughput(total_ops),
            error_count: error_count.load(Ordering::Relaxed),
        },
        shared_vfs,
    ))
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
