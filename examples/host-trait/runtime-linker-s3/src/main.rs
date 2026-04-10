//! Runtime Linker with S3 Sync
//!
//! This example demonstrates using vfs-host with S3 synchronization enabled.
//! Files written by WASM applications are automatically synced to S3.
//!
//! ## Environment Variables
//!
//! Required:
//! - `VFS_S3_BUCKET`: S3 bucket name
//!
//! Optional:
//! - `VFS_S3_PREFIX`: Key prefix (default: "vfs/")
//! - `VFS_SYNC_MODE`: "batch" (default) or "realtime"
//! - `AWS_ENDPOINT_URL`: Custom S3 endpoint (for LocalStack/MinIO)
//! - `AWS_REGION`: AWS region
//!
//! ## Usage
//!
//! ```bash
//! # With LocalStack
//! docker run -d -p 4566:4566 localstack/localstack
//! aws --endpoint-url=http://localhost:4566 s3 mb s3://test-bucket
//!
//! # Run the example (from examples/host-trait/runtime-linker-s3/)
//! VFS_S3_BUCKET=test-bucket \
//! AWS_ENDPOINT_URL=http://localhost:4566 \
//! cargo run
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

use vfs_host::{self, VfsHostState};

/// Resolve the workspace root (3 levels above CARGO_MANIFEST_DIR).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("failed to resolve workspace root")
}

/// Initialize VFS with S3 sync (async operation)
async fn init_vfs_with_s3() -> Result<VfsHostState> {
    let bucket =
        std::env::var("VFS_S3_BUCKET").context("VFS_S3_BUCKET environment variable is required")?;
    let prefix = std::env::var("VFS_S3_PREFIX").unwrap_or_else(|_| "vfs/".to_string());

    log::info!("Initializing VFS with S3 sync...");
    log::info!("  Bucket: {}", bucket);
    log::info!("  Prefix: {}", prefix);
    log::info!(
        "  Mode: {}",
        std::env::var("VFS_SYNC_MODE").unwrap_or_else(|_| "batch".to_string())
    );

    VfsHostState::new_with_s3(bucket, prefix)
        .await
        .context("Failed to create VfsHostState with S3")
}

/// Run WASM applications (sync operation - must run outside tokio runtime)
fn run_wasm(engine: &Engine, vfs_host_state: VfsHostState) -> Result<VfsHostState> {
    log::info!("VFS initialized, running WASM applications...");

    // Try to load demo-writer if available
    // Note: cargo run executes from workspace root
    let writer_path = workspace_root().join("target/wasm32-wasip2/debug/demo-writer.wasm");
    if writer_path.exists() {
        log::info!("Running demo-writer...");

        let writer_state = vfs_host_state
            .clone_shared_with_args(&["demo-writer", "/message.txt", "Hello from App1!"]);
        let mut store = Store::new(engine, writer_state);
        let mut linker = wasmtime::component::Linker::new(engine);
        vfs_host::add_to_linker_with_vfs(&mut linker)?;

        let writer_component = Component::from_file(engine, &writer_path)
            .context("Failed to load demo-writer.wasm")?;

        use wasmtime_wasi::bindings::sync::Command;
        let writer_command = Command::instantiate(&mut store, &writer_component, &linker)
            .context("Failed to instantiate demo-writer")?;

        match writer_command.wasi_cli_run().call_run(&mut store) {
            Ok(Ok(())) => log::info!("demo-writer executed successfully"),
            Ok(Err(())) => log::error!("demo-writer returned error"),
            Err(e) => log::error!("demo-writer execution failed: {:?}", e),
        }
    } else {
        log::warn!(
            "demo-writer.wasm not found at {}. Build it first with: cargo build -p demo-writer --target wasm32-wasip2",
            writer_path.display()
        );
    }

    // Return the original state (with sync thread) so we can wait for sync
    Ok(vfs_host_state)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("=== VFS Host S3 Sync Example ===");

    // Check for required environment variable
    if std::env::var("VFS_S3_BUCKET").is_err() {
        eprintln!("Error: VFS_S3_BUCKET environment variable is required");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  VFS_S3_BUCKET=my-bucket cargo run");
        eprintln!();
        eprintln!("With LocalStack:");
        eprintln!("  docker run -d -p 4566:4566 localstack/localstack");
        eprintln!("  aws --endpoint-url=http://localhost:4566 s3 mb s3://test-bucket");
        eprintln!("  VFS_S3_BUCKET=test-bucket AWS_ENDPOINT_URL=http://localhost:4566 \\");
        eprintln!("    cargo run");
        std::process::exit(1);
    }

    // Create wasmtime engine
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);

    // Initialize VFS with S3 (async)
    let vfs_host_state = init_vfs_with_s3().await?;

    // Run WASM in a blocking thread (wasmtime-wasi sync doesn't work in async context)
    let engine_clone = Arc::clone(&engine);
    let vfs_host_state =
        tokio::task::spawn_blocking(move || run_wasm(&engine_clone, vfs_host_state))
            .await
            .context("WASM execution thread panicked")??;

    // Wait for batch sync to flush
    log::info!("Waiting for sync to complete...");
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

    // Graceful shutdown - VfsHostState will be dropped here, triggering final flush
    log::info!("Shutting down (final sync will be performed)...");
    drop(vfs_host_state);

    log::info!("Done!");
    Ok(())
}
