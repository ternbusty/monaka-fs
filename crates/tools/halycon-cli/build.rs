use std::path::PathBuf;
use std::{env, fs};

/// WASM binaries to embed in the CLI.
/// (env var name, source path relative to repo root, build hint for error message)
const WASM_BINARIES: &[(&str, &str, &str)] = &[
    (
        "VFS_ADAPTER_WASM",
        "target/wasm32-wasip2/release/vfs_adapter.wasm",
        "cargo build --release --target wasm32-wasip2 -p vfs-adapter",
    ),
    (
        "VFS_ADAPTER_S3_WASM",
        "target/wasm32-wasip2/s3-release/vfs_adapter.wasm",
        "cargo build --release --target wasm32-wasip2 -p vfs-adapter --features s3-sync\n       Then copy to target/wasm32-wasip2/s3-release/vfs_adapter.wasm",
    ),
    (
        "RPC_ADAPTER_WASM",
        "target/wasm32-wasip2/release/rpc_adapter.wasm",
        "cargo build --release --target wasm32-wasip2 -p rpc-adapter",
    ),
    (
        "VFS_RPC_SERVER_WASM",
        "target/wasm32-wasip2/release/vfs_rpc_server.wasm",
        "cargo build --release --target wasm32-wasip2 -p vfs-rpc-server",
    ),
    (
        "VFS_RPC_SERVER_S3_WASM",
        "target/wasm32-wasip2/s3-release/vfs_rpc_server.wasm",
        "cargo build --release --target wasm32-wasip2 -p vfs-rpc-server --features s3-sync\n       Then copy to target/wasm32-wasip2/s3-release/vfs_rpc_server.wasm",
    ),
];

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo_root = manifest_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("crates").exists())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| manifest_dir.clone());

    let mut missing = Vec::new();

    for &(env_name, rel_path, build_hint) in WASM_BINARIES {
        let source = repo_root.join(rel_path);
        let dest = out_dir.join(format!("{}.wasm", env_name.to_lowercase()));

        if source.exists() {
            fs::copy(&source, &dest).expect("Failed to copy WASM file");
        } else {
            missing.push((env_name, build_hint));
            continue;
        }

        println!("cargo:rustc-env={}={}", env_name, dest.display());
        println!("cargo:rerun-if-changed={}", source.display());
    }

    if !missing.is_empty() {
        let mut msg = String::from("Missing WASM binaries required to build halycon CLI:\n\n");
        for (name, hint) in &missing {
            msg.push_str(&format!("  {name}:\n       {hint}\n\n"));
        }
        panic!("{}", msg);
    }
}
