use std::path::PathBuf;

use anyhow::{Context, Result};
use wac_graph::types::Package;
use wac_graph::{CompositionGraph, EncodeOptions, plug};

use crate::{embed, wasm};

pub fn run(
    app: &PathBuf,
    output: &PathBuf,
    rpc: bool,
    s3_sync: bool,
    mounts: &[String],
) -> Result<()> {
    let adapter_bytes = if rpc {
        wasm::rpc_adapter().to_vec()
    } else {
        let base = wasm::vfs_adapter(s3_sync);
        if mounts.is_empty() {
            base.to_vec()
        } else {
            let parsed_mounts: Vec<(String, PathBuf)> = mounts
                .iter()
                .map(|m| embed::parse_mount(m))
                .collect::<Result<Vec<_>>>()?;
            embed::embed_into_bytes(base, &parsed_mounts)?
        }
    };

    let app_bytes = std::fs::read(app)
        .with_context(|| format!("Failed to read app WASM: {}", app.display()))?;

    // Build composition graph and plug adapter into app
    let mut graph: CompositionGraph = CompositionGraph::new();

    let adapter_package = Package::from_bytes("adapter", None, adapter_bytes, graph.types_mut())?;
    let adapter_id = graph.register_package(adapter_package)?;

    let app_package = Package::from_bytes("app", None, app_bytes, graph.types_mut())?;
    let app_id = graph.register_package(app_package)?;

    plug(&mut graph, vec![adapter_id], app_id)?;

    let composed = graph.encode(EncodeOptions::default())?;

    std::fs::write(output, &composed)?;
    println!("Composed {} ({} bytes)", output.display(), composed.len());

    Ok(())
}
