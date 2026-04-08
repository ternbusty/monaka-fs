# Host Trait: Runtime Linker (demo-fs-operations)

Native host binary that runs `demo-fs-operations` via Wasmtime's `Linker` API with `vfs-host`. Exercises all filesystem operations including rename/move.

## Build


```bash
# Build the WASM app
cargo build -p demo-fs-operations --target wasm32-wasip2

# Build the host binary (standalone package)
cd examples/host-trait/runtime-linker-demo-fs
cargo build
```

## Run

```bash
# From examples/host-trait/runtime-linker-demo-fs/:
cargo run
```
