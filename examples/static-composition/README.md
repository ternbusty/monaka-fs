# Static Composition Examples

Build-time composition: combine a WASM application with `vfs-adapter` into a single component.

```
App (std::fs) + vfs-adapter  -->  Composed WASM
```

## Quick Start with `monaka` CLI

```bash
# From repository root:

# Build CLI and demo app
make build-cli
cargo build -p demo-fs-operations --target wasm32-wasip2

# Compose with vfs-adapter
target/release/monaka compose \
  target/wasm32-wasip2/debug/demo-fs-operations.wasm \
  -o /tmp/composed-demo-fs-operations.wasm

# Run
wasmtime run /tmp/composed-demo-fs-operations.wasm
```

Other options:

```bash
# With file embedding
target/release/monaka compose --mount /data=./local-dir app.wasm -o composed.wasm

# With S3 sync
target/release/monaka compose --s3-sync app.wasm -o composed.wasm
```

## Examples

| Directory | Description |
|-----------|-------------|
| [embed/](./embed/) | File embedding into vfs-adapter |
| [c/](./c/) | C application using standard C I/O functions |
| [s3-sync/](./s3-sync/) | Rust application with automatic S3 syncing |

## Manual Setup (without `monaka` CLI)

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
