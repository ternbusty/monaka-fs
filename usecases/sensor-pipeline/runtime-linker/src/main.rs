//! Sensor Pipeline Runner
//!
//! Demonstrates two WASM applications sharing an in-memory VFS:
//! - sensor-ingest: Collects sensor data, writes to /data/sensor.log
//! - sensor-process: Reads log, performs statistical analysis

use anyhow::{Context, Result};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

fn run_sensor_pipeline(engine: &Engine, vfs_adapter_path: &str) -> Result<()> {
    println!("=== VFS Sharing Demo: Sensor Data Pipeline ===");
    println!();
    println!("Demonstrating data pipeline between two WASM applications:");
    println!("  1. sensor-ingest: Collects sensor data, writes to /data/sensor.log");
    println!("  2. sensor-process: Reads log, performs statistical analysis");
    println!();

    // Create shared VfsHostState
    let vfs_host_state1 = vfs_host::VfsHostState::new(engine, vfs_adapter_path)
        .context("Failed to create VfsHostState")?;
    let vfs_host_state2 = vfs_host_state1.clone_shared();

    // Run sensor-ingest
    println!("--- Running sensor-ingest ---");
    let mut store1 = Store::new(engine, vfs_host_state1);
    let mut linker1 = wasmtime::component::Linker::new(engine);
    vfs_host::add_to_linker_with_vfs(&mut linker1)?;

    let ingest_path = "../../../target/wasm32-wasip2/debug/sensor-ingest.wasm";
    let ingest_component =
        Component::from_file(engine, ingest_path).context("Failed to load sensor-ingest.wasm")?;

    use wasmtime_wasi::bindings::sync::Command;
    let ingest_command = Command::instantiate(&mut store1, &ingest_component, &linker1)
        .context("Failed to instantiate sensor-ingest")?;

    match ingest_command.wasi_cli_run().call_run(&mut store1) {
        Ok(Ok(())) => {}
        Ok(Err(())) => return Err(anyhow::anyhow!("sensor-ingest returned error")),
        Err(e) => return Err(anyhow::anyhow!("sensor-ingest execution failed: {:?}", e)),
    }

    // Run sensor-process (shares same VFS)
    println!();
    println!("--- Running sensor-process ---");
    let mut store2 = Store::new(engine, vfs_host_state2);
    let mut linker2 = wasmtime::component::Linker::new(engine);
    vfs_host::add_to_linker_with_vfs(&mut linker2)?;

    let process_path = "../../../target/wasm32-wasip2/debug/sensor-process.wasm";
    let process_component =
        Component::from_file(engine, process_path).context("Failed to load sensor-process.wasm")?;

    let process_command = Command::instantiate(&mut store2, &process_component, &linker2)
        .context("Failed to instantiate sensor-process")?;

    match process_command.wasi_cli_run().call_run(&mut store2) {
        Ok(Ok(())) => {}
        Ok(Err(())) => return Err(anyhow::anyhow!("sensor-process returned error")),
        Err(e) => return Err(anyhow::anyhow!("sensor-process execution failed: {:?}", e)),
    }

    println!();
    println!("=== Demo Complete ===");
    println!("  sensor-ingest wrote data to shared VFS");
    println!("  sensor-process read and analyzed data from shared VFS");

    Ok(())
}

fn main() -> Result<()> {
    let vfs_adapter_path = "../../../target/wasm32-wasip2/debug/vfs_adapter.wasm";

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    run_sensor_pipeline(&engine, vfs_adapter_path)?;

    Ok(())
}
