# Static Composition: C Example

C application using standard C I/O (`stdio.h`, `unistd.h`, `dirent.h`), compiled to WASM and composed with `vfs-adapter`.

## Using `halycon` CLI

```bash
# Build the C component (standalone package, must build from its directory)
cd examples/static-composition/c
cargo build --target wasm32-wasip2
cd ../../..

# Compose with halycon
target/release/halycon compose \
  examples/static-composition/c/target/wasm32-wasip2/debug/component_c.wasm \
  -o /tmp/composed-c.wasm

# Run
wasmtime run /tmp/composed-c.wasm
```

## Expected Output

4 test suites (basic file operations, directory operations, metadata operations, error handling), implemented in C using standard C I/O functions.

## Manual Setup (without `halycon` CLI)

### Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

[WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases) is required for C compilation. Extract to one of the following paths (auto-detected by `build.rs`):

- `~/wasi-sdk`
- `/opt/wasi-sdk`
- `/usr/local/opt/wasi-sdk`

### Build & Compose

```bash
# From repository root:

# Build the VFS adapter
cargo build -p vfs-adapter --target wasm32-wasip2

# Build the C component
cd examples/static-composition/c
cargo build --target wasm32-wasip2
cd ../../..

# Compose
wac plug \
  --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
  examples/static-composition/c/target/wasm32-wasip2/debug/component_c.wasm \
  -o /tmp/composed-c.wasm

# Run
wasmtime run /tmp/composed-c.wasm
```
