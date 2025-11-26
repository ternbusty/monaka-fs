//! RPC Filesystem Host - Custom Wasmtime host that uses RPC for filesystem operations
//!
//! This host program runs WASM components that use std::fs transparently,
//! with all filesystem operations routed through an RPC server.

use anyhow::{Context, Result};
use std::path::PathBuf;
use vfs_rpc_host::VfsRpcHostState;
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::sync::Command;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <server-address> <wasm-component-path>", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!(
            "  {} localhost:9000 target/wasm32-wasip2/debug/demo_std_fs.wasm",
            args[0]
        );
        std::process::exit(1);
    }

    let server_addr = &args[1];
    let wasm_path = PathBuf::from(&args[2]);

    // Create Wasmtime engine with component model (no async support needed)
    let mut config = Config::new();
    config.wasm_component_model(true);

    let engine = Engine::new(&config)?;

    // Create VFS RPC host state by connecting to RPC server
    println!("Connecting to VFS RPC server at {}...", server_addr);
    let host_state =
        VfsRpcHostState::new(server_addr).context("Failed to connect to RPC server")?;
    println!("Connected!");

    let mut store = Store::new(&engine, host_state);

    // Load the component
    println!("Loading WASM component: {}", wasm_path.display());
    let component_bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read {}", wasm_path.display()))?;

    let component =
        Component::from_binary(&engine, &component_bytes).context("Failed to compile component")?;

    // Instantiate and link with sync WASI bindings (we implement sync filesystem Host traits)
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker_sync(&mut linker)?;

    println!("Instantiating component...");

    // Instantiate the component
    let instance = linker
        .instantiate(&mut store, &component)
        .context("Failed to instantiate component")?;

    // Create Command from the instance
    let command =
        Command::new(&mut store, &instance).context("Failed to create Command from instance")?;

    println!("Running component...");

    // Call the wasi:cli/run interface
    let result = command.wasi_cli_run().call_run(&mut store)?;

    match result {
        Ok(()) => {
            println!("Component execution completed successfully");
            Ok(())
        }
        Err(()) => {
            eprintln!("Component returned error");
            std::process::exit(1);
        }
    }
}
