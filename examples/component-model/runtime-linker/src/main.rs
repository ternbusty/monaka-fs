use anyhow::{Context, Result};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

// Import VFS Host trait implementations from vfs-host crate
use vfs_host::{self};

fn test_shared_vfs_across_apps(engine: &Engine, vfs_adapter_path: &str) -> Result<()> {
    println!("Demonstrating that multiple WASM applications can share the same VFS instance.");
    println!("App1 (demo-writer) creates a file, App2 (demo-reader) reads it.");
    println!();

    // Create shared VfsHostState
    println!("Creating shared VfsHostState...");
    let vfs_host_state1 = vfs_host::VfsHostState::new(engine, vfs_adapter_path)
        .context("Failed to create VfsHostState")?;
    let vfs_host_state2 = vfs_host_state1.clone_shared();

    // Run demo-writer (App1)
    println!();
    println!("Running demo-writer (App1)...");

    let mut store1 = Store::new(engine, vfs_host_state1);
    let mut linker1 = wasmtime::component::Linker::new(engine);
    vfs_host::add_to_linker_with_vfs(&mut linker1)?;

    let writer_path = "../../../target/wasm32-wasip2/debug/demo-writer.wasm";
    let writer_component =
        Component::from_file(engine, writer_path).context("Failed to load demo-writer.wasm")?;

    use wasmtime_wasi::bindings::sync::Command;
    let writer_command = Command::instantiate(&mut store1, &writer_component, &linker1)
        .context("Failed to instantiate demo-writer")?;

    match writer_command.wasi_cli_run().call_run(&mut store1) {
        Ok(Ok(())) => println!("demo-writer executed successfully"),
        Ok(Err(())) => return Err(anyhow::anyhow!("demo-writer returned error")),
        Err(e) => return Err(anyhow::anyhow!("demo-writer execution failed: {:?}", e)),
    }

    // Run demo-reader (App2) with shared VFS
    println!();
    println!("Running demo-reader (App2) with shared VFS...");

    let mut store2 = Store::new(engine, vfs_host_state2);
    let mut linker2 = wasmtime::component::Linker::new(engine);
    vfs_host::add_to_linker_with_vfs(&mut linker2)?;

    let reader_path = "../../../target/wasm32-wasip2/debug/demo-reader.wasm";
    let reader_component =
        Component::from_file(engine, reader_path).context("Failed to load demo-reader.wasm")?;

    let reader_command = Command::instantiate(&mut store2, &reader_component, &linker2)
        .context("Failed to instantiate demo-reader")?;

    match reader_command.wasi_cli_run().call_run(&mut store2) {
        Ok(Ok(())) => println!("demo-reader executed successfully"),
        Ok(Err(())) => return Err(anyhow::anyhow!("demo-reader returned error")),
        Err(e) => return Err(anyhow::anyhow!("demo-reader execution failed: {:?}", e)),
    }

    Ok(())
}

fn main() -> Result<()> {
    // File paths
    let vfs_adapter_path = "../../../target/wasm32-wasip2/debug/vfs_adapter.wasm";

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // Shared VFS across multiple WASM applications
    test_shared_vfs_across_apps(&engine, vfs_adapter_path)?;

    Ok(())
}
