# Static Composition Examples

Build-time composition: combine a WASM application with `vfs-adapter` into a single component.

```
App (std::fs) + vfs-adapter  -->  Composed WASM
```

## Quick Start with `halycon` CLI

```bash
# Install (from repository root)
make build-cli

# Compose any app with vfs-adapter
halycon compose my-app.wasm -o composed.wasm

# With file embedding
halycon compose --mount /data=./local-dir my-app.wasm -o composed.wasm

# With S3 sync
halycon compose --s3-sync my-app.wasm -o composed.wasm

# Run
wasmtime run composed.wasm
```

## Examples

| Directory | Description |
|-----------|-------------|
| [embed/](./embed/) | File embedding into vfs-adapter |
| [c/](./c/) | C application using standard C I/O functions |
| [s3-sync/](./s3-sync/) | Rust application with automatic S3 syncing |

## Manual Setup (without `halycon` CLI)

Any WASM app from [apps/](../apps/) can also be composed manually with `wac plug`:

```bash
# Prerequisites
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli

# From repository root:
cargo build -p vfs-adapter --target wasm32-wasip2
cargo build -p demo-fs-operations --target wasm32-wasip2
wac plug \
  --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
  target/wasm32-wasip2/debug/demo-fs-operations.wasm \
  -o target/wasm32-wasip2/debug/composed-demo-fs-operations.wasm
wasmtime run target/wasm32-wasip2/debug/composed-demo-fs-operations.wasm
```

See each subdirectory's README for details.
