//! Benchmark Runtime: vfs-host with S3 Passthrough
//!
//! Runs WASM benchmark app using vfs-host with S3 synchronization.
//! Supports full S3 passthrough mode (realtime + read-through + metadata sync).

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

use vfs_host::{self, VfsHostState};

/// Initialize VFS with S3 sync (async operation)
async fn init_vfs_with_s3() -> Result<VfsHostState> {
    let bucket = std::env::var("VFS_S3_BUCKET")
        .context("VFS_S3_BUCKET environment variable is required")?;
    let prefix = std::env::var("VFS_S3_PREFIX").unwrap_or_else(|_| "vfs/".to_string());

    log::info!("Initializing VFS with S3 sync...");
    log::info!("  Bucket: {}", bucket);
    log::info!("  Prefix: {}", prefix);
    log::info!(
        "  Sync Mode: {}",
        std::env::var("VFS_SYNC_MODE").unwrap_or_else(|_| "batch".to_string())
    );
    log::info!(
        "  Read Mode: {}",
        std::env::var("VFS_READ_MODE").unwrap_or_else(|_| "local".to_string())
    );
    log::info!(
        "  Metadata Mode: {}",
        std::env::var("VFS_METADATA_MODE").unwrap_or_else(|_| "local".to_string())
    );

    VfsHostState::new_with_s3(bucket, prefix)
        .await
        .context("Failed to create VfsHostState with S3")
}

/// Run WASM benchmark (sync operation - must run outside tokio runtime)
fn run_wasm(engine: &Engine, vfs_host_state: VfsHostState, wasm_path: &str) -> Result<VfsHostState> {
    let mut store = Store::new(engine, vfs_host_state);
    let mut linker = wasmtime::component::Linker::new(engine);
    vfs_host::add_to_linker_with_vfs(&mut linker)?;

    log::info!("Loading WASM: {}", wasm_path);
    let component = Component::from_file(engine, wasm_path)
        .context("Failed to load WASM component")?;

    use wasmtime_wasi::bindings::sync::Command;
    let command = Command::instantiate(&mut store, &component, &linker)
        .context("Failed to instantiate WASM")?;

    log::info!("Running benchmark...");
    match command.wasi_cli_run().call_run(&mut store) {
        Ok(Ok(())) => log::info!("Benchmark completed successfully"),
        Ok(Err(())) => log::error!("Benchmark returned error"),
        Err(e) => log::error!("Benchmark execution failed: {:?}", e),
    }

    Ok(store.into_data())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <wasm-path>", args[0]);
        eprintln!();
        eprintln!("Environment variables:");
        eprintln!("  VFS_S3_BUCKET      - S3 bucket name (required)");
        eprintln!("  VFS_S3_PREFIX      - S3 key prefix (default: vfs/)");
        eprintln!("  VFS_SYNC_MODE      - batch or realtime (default: batch)");
        eprintln!("  VFS_READ_MODE      - local or s3 (default: local)");
        eprintln!("  VFS_METADATA_MODE  - local or s3 (default: local)");
        eprintln!("  AWS_ENDPOINT_URL   - Custom S3 endpoint (for LocalStack)");
        eprintln!("  AWS_REGION         - AWS region");
        std::process::exit(1);
    }

    let wasm_path = &args[1];

    // Check for required environment variable
    if std::env::var("VFS_S3_BUCKET").is_err() {
        eprintln!("Error: VFS_S3_BUCKET environment variable is required");
        std::process::exit(1);
    }

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);

    // Initialize VFS with S3 (async)
    let vfs_host_state = init_vfs_with_s3().await?;

    // Run WASM benchmark
    let engine_clone = Arc::clone(&engine);
    let wasm_path_clone = wasm_path.clone();
    let bench_start = Instant::now();

    let vfs_host_state = tokio::task::spawn_blocking(move || {
        run_wasm(&engine_clone, vfs_host_state, &wasm_path_clone)
    })
    .await
    .context("WASM execution thread panicked")??;

    let bench_elapsed = bench_start.elapsed();
    log::info!("WASM execution time: {:.3}ms", bench_elapsed.as_secs_f64() * 1000.0);

    // For realtime mode, no need to wait for batch flush
    let sync_mode = std::env::var("VFS_SYNC_MODE").unwrap_or_else(|_| "batch".to_string());
    if sync_mode == "batch" {
        log::info!("Waiting for batch sync to flush...");
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    }

    // Graceful shutdown - triggers final S3 sync
    log::info!("Shutting down (final sync)...");
    let shutdown_start = Instant::now();
    drop(vfs_host_state);
    let shutdown_elapsed = shutdown_start.elapsed();
    log::info!("Shutdown time: {:.3}ms", shutdown_elapsed.as_secs_f64() * 1000.0);

    let total_elapsed = bench_start.elapsed();
    println!("[TOTAL_WITH_SYNC] {:.3}ms", total_elapsed.as_secs_f64() * 1000.0);

    Ok(())
}
