//! RPC Filesystem Runner
//!
//! This host program runs WASM applications with filesystem operations routed
//! through vfs-rpc-host, which uses the rpc-adapter component to forward
//! operations over TCP RPC to vfs-rpc-server.
//!
//! Architecture:
//! Application (demo-std-fs.wasm) → vfs-rpc-host → rpc-adapter.wasm → TCP RPC → vfs-rpc-server → fs-core

use anyhow::{Context, Result};
use std::path::PathBuf;
use vfs_rpc_host::VfsRpcHostState;
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::sync::Command;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <wasm-app-path> [rpc-adapter-path]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} target/wasm32-wasip2/debug/demo-std-fs.wasm", args[0]);
        eprintln!(
            "  {} target/wasm32-wasip2/debug/demo-std-fs.wasm target/wasm32-wasip2/debug/rpc_adapter.wasm",
            args[0]
        );
        std::process::exit(1);
    }

    let wasm_app_path = PathBuf::from(&args[1]);
    let rpc_adapter_path = if args.len() >= 3 {
        args[2].clone()
    } else {
        // Default path to rpc-adapter component
        "target/wasm32-wasip2/debug/rpc_adapter.wasm".to_string()
    };

    println!("=== RPC Filesystem Runner ===");
    println!("Application: {}", wasm_app_path.display());
    println!("RPC Adapter: {}", rpc_adapter_path);
    println!();

    // Create Wasmtime engine with component model support (sync mode)
    let mut config = Config::new();
    config.wasm_component_model(true);

    let engine = Engine::new(&config)?;

    // Create VfsRpcHostState which wraps the rpc-adapter component
    println!("Loading RPC adapter component...");
    let host_state = VfsRpcHostState::new(&engine, &rpc_adapter_path)
        .context("Failed to create VfsRpcHostState")?;

    println!("RPC adapter loaded successfully");
    println!();

    let mut store = Store::new(&engine, host_state);

    // Load the application component
    println!("Loading application component: {}", wasm_app_path.display());
    let component_bytes = std::fs::read(&wasm_app_path)
        .with_context(|| format!("Failed to read {}", wasm_app_path.display()))?;

    let component =
        Component::from_binary(&engine, &component_bytes).context("Failed to compile component")?;

    // Create linker and add WASI support
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker_sync(&mut linker)?;

    println!("Instantiating application component...");

    // Instantiate the application component
    let instance = linker
        .instantiate(&mut store, &component)
        .context("Failed to instantiate component")?;

    // Create Command from the instance
    let command =
        Command::new(&mut store, &instance).context("Failed to create Command from instance")?;

    println!("Running application...");
    println!();

    // Call the wasi:cli/run interface
    let result = command
        .wasi_cli_run()
        .call_run(&mut store)
        .context("Failed to call run function")?;

    println!();
    match result {
        Ok(()) => {
            println!("Application completed successfully");
            Ok(())
        }
        Err(()) => {
            eprintln!("Application returned error");
            std::process::exit(1);
        }
    }
}
