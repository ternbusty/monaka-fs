# Static Composition Examples

Build-time composition using `wac plug` to combine a WASM application with `vfs-adapter` into a single component.

```
App (std::fs) + vfs-adapter  --[wac plug]-->  Composed WASM
```

## Examples

| Directory | Description |
|-----------|-------------|
| [c/](./c/) | C application using standard C I/O functions |
| [s3-sync/](./s3-sync/) | Rust application with automatic S3 syncing |

Any WASM app from [apps/](../apps/) can also be composed with `vfs-adapter`. For example:

```bash
# From repository root:
cargo build -p vfs-adapter --target wasm32-wasip2
cargo build -p demo-fs-operations --target wasm32-wasip2
wac plug \
  --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
  target/wasm32-wasip2/debug/demo-fs-operations.wasm \
  -o target/wasm32-wasip2/debug/composed-demo-fs-operations.wasm
wasmtime run target/wasm32-wasip2/debug/composed-demo-fs-operations.wasm
```

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

See each subdirectory's README for details.
