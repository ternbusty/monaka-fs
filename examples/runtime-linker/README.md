# Runtime Dynamic Linking

This directory demonstrates runtime dynamic linking with WebAssembly Component Model.

## Overview

The Component Model supports two linking approaches:

1. **Dynamic Linking** (this approach):
   - Components are loaded separately at runtime
   - Linked dynamically using `wac plug` or Wasmtime's Linker API
   - Allows swapping implementations without rebuilding

2. **Static Composition** (alternative - use `make demo-static`):
   - Components are linked at build time using `wac plug`
   - Produces a single `.composed.wasm` file
   - Simple deployment: one file to distribute

## What This Demo Shows

This program demonstrates:

### Part 1: Separate Component Loading
- Loads VFS Adapter component (4.24 MB)
- Loads Application component (2.46 MB)
- Shows components can be distributed and loaded independently

### Part 2: Runtime Composition
- Uses `wac plug` to compose components at runtime
- Measures composition time (~13ms)
- Executes the composed application

### Part 3: Comparison
- Compares static vs dynamic composition
- Shows file sizes and overhead
- Analyzes trade-offs

## How to Build and Run

### Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wasmtime-cli wac-cli
```

### Quick Start

From the repository root or examples directory:

```bash
make demo
# or
make demo-dynamic
```

This automatically:
1. Builds the VFS adapter component
2. Builds the component-rust application
3. Builds the runtime linker host program
4. Runs the dynamic linking demo

### Manual Build and Run

```bash
# From repository root:
make build-vfs-adapter       # Build VFS adapter
make build-component-rust    # Build application
make build-runtime-linker    # Build runtime linker host

# Run the demo
cd examples/runtime-linker
cargo run --release
```

## Technical Details

### Component Files

- **VFS Adapter**: `../../target/wasm32-wasip2/debug/vfs_adapter.wasm`
  - Exports `wasi:filesystem/types@0.2.6` and other WASI interfaces
  - Provides in-memory filesystem implementation

- **Application**: `../component-rust/target/wasm32-wasip2/debug/component-rust.wasm`
  - Imports `wasi:filesystem/types@0.2.6` and other WASI interfaces
  - Uses standard Rust `std::fs` API

### Runtime Composition Process

1. Load components separately using `wasmtime::component::Component::from_file()`
2. Compose using `wac plug`: Links imports to exports
3. Execute composed component using `wasmtime run`

### Alternative: Wasmtime Linker API

This demo uses `wac plug` for composition. An alternative is Wasmtime's `Linker` API:

```rust
let mut linker = Linker::new(&engine);
let vfs_instance = linker.instantiate(&mut store, &vfs_adapter)?;
// Register VFS exports in linker...
let app_instance = linker.instantiate(&mut store, &app_component)?;
```

However, this is more complex and requires manual interface wiring.

## Performance

Based on measurements:
- Component loading: ~24ms per component
- Runtime composition: ~14ms
- Execution: ~84ms
- Total overhead: ~48ms vs static composition
