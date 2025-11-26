//! WASM Runner - Custom Wasmtime host with network permissions
//!
//! This host program runs WASM components with WASI Preview 2 support
//! and full network permissions enabled.

use anyhow::{Context, Result};
use std::path::PathBuf;
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::Command;
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

struct Host {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl WasiView for Host {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <wasm-component-path>", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!(
            "  {} target/wasm32-wasip2/debug/vfs_rpc_server.wasm",
            args[0]
        );
        std::process::exit(1);
    }

    let wasm_path = PathBuf::from(&args[1]);

    // Create Wasmtime engine with component model and async support
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);

    let engine = Engine::new(&config)?;

    // Create WASI context with network permissions
    let table = ResourceTable::new();
    let wasi = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_network() // Enable network access
        .build();

    let mut store = Store::new(&engine, Host { wasi, table });

    // Load the component
    println!("Loading WASM component: {}", wasm_path.display());
    let component_bytes = std::fs::read(&wasm_path)
        .with_context(|| format!("Failed to read {}", wasm_path.display()))?;

    let component =
        Component::from_binary(&engine, &component_bytes).context("Failed to compile component")?;

    // Instantiate and link
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::add_to_linker_async(&mut linker)?;

    println!("Instantiating component...");

    // Instantiate the component
    let instance = linker
        .instantiate_async(&mut store, &component)
        .await
        .context("Failed to instantiate component")?;

    // Create Command from the instance
    let command =
        Command::new(&mut store, &instance).context("Failed to create Command from instance")?;

    println!("Running component...");

    // Call the wasi:cli/run interface
    let result = command
        .wasi_cli_run()
        .call_run(&mut store)
        .await
        .context("Failed to call run function")?;

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
