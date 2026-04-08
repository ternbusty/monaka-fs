# Host Trait: Runtime Linker (demo-fs-operations)

Native host binary that runs `demo-fs-operations` via Wasmtime's `Linker` API with `vfs-host`. Exercises all filesystem operations including rename/move.

## Build

All commands from the repository root.

```bash
# Build the WASM app
cargo build -p demo-fs-operations --target wasm32-wasip2

# Build the host binary
cargo build -p runtime-linker-demo-fs
```

## Run

```bash
cargo run -p runtime-linker-demo-fs
```
