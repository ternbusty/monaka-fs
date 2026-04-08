# Host Trait: Runtime Linker

Native host binary that loads WASM applications at runtime and provides the VFS via Wasmtime's `Linker` API. Demonstrates VFS sharing between multiple WASM apps through `VfsHostState::clone_shared()`.

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wasmtime-cli
```

## Build

All commands from the repository root.

```bash
# Build the WASM apps that will be loaded (see examples/apps/)
cargo build -p demo-writer --target wasm32-wasip2
cargo build -p demo-reader --target wasm32-wasip2

# Build the host binary (standalone package)
cd examples/host-trait/runtime-linker
cargo build
```

## Run

```bash
# From examples/host-trait/runtime-linker/:
cargo run
```

## Expected Output

```
Demonstrating that multiple WASM applications can share the same VFS instance.
App1 (demo-writer) creates a file, App2 (demo-reader) reads it.

Creating shared VfsHostState...

Running demo-writer (App1)...
=== VFS Demo App 1: File Writer ===

Writing file: /message.txt
  Wrote 16 bytes

=== App1 completed successfully ===
demo-writer executed successfully

Running demo-reader (App2) with shared VFS...
=== VFS Demo App 2: File Reader ===

Getting file metadata: /message.txt
  File size: 16 bytes

Reading file: /message.txt
  Content (16 bytes):
  "Hello from App1!"

=== App2 completed ===
demo-reader executed successfully
```
