use std::path::PathBuf;

use anyhow::Result;

use crate::wasm;

pub fn run(name: &str, output: &PathBuf, s3_sync: bool) -> Result<()> {
    let bytes = wasm::get_binary(name, s3_sync)?;
    std::fs::write(output, bytes)?;
    println!("Wrote {} ({} bytes)", output.display(), bytes.len());
    Ok(())
}
