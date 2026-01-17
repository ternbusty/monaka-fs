//! Benchmark Runner for Host Trait Method
//!
//! Runs the benchmark WASM using vfs-host (native wasmtime + host trait).
//! This represents the "host trait" method in the benchmark comparison.

use anyhow::{Context, Result};
use std::env;
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let wasm_path = if args.len() > 1 {
        &args[1]
    } else {
        // Default path relative to bench-runner directory
        "../bench-app/target/wasm32-wasip2/release/bench-all-methods.wasm"
    };

    // Create engine with component model enabled
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // Create VfsHostState (uses fs-core directly via host trait)
    let vfs_host_state = vfs_host::VfsHostState::new()
        .context("Failed to create VfsHostState")?;

    // Create store and linker
    let mut store = Store::new(&engine, vfs_host_state);
    let mut linker = wasmtime::component::Linker::new(&engine);
    vfs_host::add_to_linker_with_vfs(&mut linker)?;

    // Load and instantiate the benchmark component
    let component = Component::from_file(&engine, wasm_path)
        .with_context(|| format!("Failed to load WASM: {}", wasm_path))?;

    use wasmtime_wasi::bindings::sync::Command;
    let command = Command::instantiate(&mut store, &component, &linker)
        .context("Failed to instantiate component")?;

    // Run the benchmark
    match command.wasi_cli_run().call_run(&mut store) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(anyhow::anyhow!("Benchmark returned error")),
        Err(e) => Err(anyhow::anyhow!("Benchmark execution failed: {:?}", e)),
    }
}
