use anyhow::{Context, Result};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

use vfs_host::{self};

fn main() -> Result<()> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    let vfs_host_state = vfs_host::VfsHostState::new().context("Failed to create VfsHostState")?;
    let mut store = Store::new(&engine, vfs_host_state);
    let mut linker = wasmtime::component::Linker::new(&engine);
    vfs_host::add_to_linker_with_vfs(&mut linker)?;

    let component = Component::from_file(
        &engine,
        "target/wasm32-wasip2/debug/demo-fs-operations.wasm",
    )
    .context("Failed to load demo-fs-operations.wasm")?;

    use wasmtime_wasi::bindings::sync::Command;
    let command = Command::instantiate(&mut store, &component, &linker)
        .context("Failed to instantiate demo-fs-operations")?;

    match command.wasi_cli_run().call_run(&mut store) {
        Ok(Ok(())) => println!("demo-fs-operations executed successfully"),
        Ok(Err(())) => return Err(anyhow::anyhow!("demo-fs-operations returned error")),
        Err(e) => return Err(anyhow::anyhow!("Execution failed: {:?}", e)),
    }

    Ok(())
}
