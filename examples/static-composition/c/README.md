# Static Composition: C Example

C application using standard C I/O (`stdio.h`, `unistd.h`, `dirent.h`), compiled to WASM and composed with `vfs-adapter` at build time via `wac plug`.

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

[WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases) is required for C compilation. Extract to one of the following paths (auto-detected by `build.rs`):

- `~/wasi-sdk`
- `/opt/wasi-sdk`
- `/usr/local/opt/wasi-sdk`

## Build

```bash
# From repository root:

# Build the VFS adapter
cargo build -p vfs-adapter --target wasm32-wasip2

# Build the C component (standalone package, must build from its directory)
cd examples/static-composition/c
cargo build --target wasm32-wasip2
cd ../../..
```

## Compose

```bash
# From repository root:
wac plug \
  --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
  examples/static-composition/c/target/wasm32-wasip2/debug/component_c.wasm \
  -o target/wasm32-wasip2/debug/composed-component-c.wasm
```

## Run

```bash
# From repository root:
wasmtime run target/wasm32-wasip2/debug/composed-component-c.wasm
```

## Expected Output

Same 4 test suites as the Rust example (basic file operations, directory operations, metadata operations, error handling), implemented in C using standard C I/O functions.
