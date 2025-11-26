# Halycon

Halycon is a virtual in-memory filesystem implemented in Rust and compiled to WebAssembly.

## Quick Start

```bash
make demo
```

## Project Structure

### Libraries

- **fs-core** - Core in-memory filesystem implementation (no_std compatible)
- **vfs-adapter** - WebAssembly Component Model VFS adapter (WASI Preview 2)
- **vfs-host** - Host trait implementation for sharing VFS across multiple applications
- **vfs-rpc-server** - TCP server exposing VFS over network (WASI Preview 2)
- **vfs-rpc-protocol** - Shared RPC protocol definitions
- **fs-wasm** - Legacy C FFI layer

## Development Setup

### Git Hooks (lefthook)

This project uses [lefthook](https://github.com/evilmartians/lefthook) for pre-commit checks.

```bash
lefthook install
```

Once installed, the following checks run automatically on `git commit`:
- `cargo fmt` - Auto-formats Rust code
- `cargo clippy` - Runs linter with warnings as errors

## How to Build

```bash
# Build libraries
make build

# Build in release mode
make build-release
```

## How to Test

```bash
# Run all tests
cargo test

# Run tests for specific package
cargo test -p fs-core
cargo test -p fs-wasm
```

## Execution Methods

### 1. Component Model - Runtime Dynamic Linking

Runtime composition using `wac plug`.

```bash
make demo
# or
make demo-dynamic
```

**Location**: `examples/runtime-linker/`

### 2. Component Model - Static Composition

Build-time composition producing single `.composed.wasm` file.

```bash
make demo-static

# Or individually:
make run-static-rust
make run-static-c
```

### 3. RPC Approach - Multiple WASM Processes

Multiple separate WASM processes sharing VFS via TCP server.

```bash
# Terminal 1: Start VFS RPC Server (port 9000)
cargo build -p wasm-runner
cargo build -p vfs-rpc-server --target wasm32-wasip2
./target/debug/wasm-runner target/wasm32-wasip2/debug/vfs_rpc_server.wasm

# Terminal 2: Run writer application
cargo build -p vfs-demo-app1 --target wasm32-wasip2
./target/debug/wasm-runner target/wasm32-wasip2/debug/vfs_demo_app1.wasm

# Terminal 3: Run reader application
cargo build -p vfs-demo-app2 --target wasm32-wasip2
./target/debug/wasm-runner target/wasm32-wasip2/debug/vfs_demo_app2.wasm
```

**Components**: `vfs-rpc-server`, `vfs-demo-app1`, `vfs-demo-app2`, `wasm-runner`

### 4. Legacy C Example

C code using fs-wasm FFI directly.

```bash
make build-c-example
make run-c-example
```

**Location**: `examples/c/`

### 5. Legacy Rust Example

Rust code using fs-core library directly.

```bash
make build-rust-example
make run-rust-example
```

**Location**: `examples/rust/`

## Component Model VFS Adapter

The VFS Adapter exports WASI Preview 2 filesystem interfaces.

### Prerequisites

```bash
# Install prerequisites
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

### Building

```bash
cargo build -p vfs-adapter --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/vfs_adapter.wasm`

### WIT Definitions

Uses official WASI Preview 2 WIT definitions (v0.2.6):
- `wit/deps/filesystem/` - WASI filesystem interfaces
- `wit/deps/io/` - WASI I/O interfaces
- `wit/deps/clocks/` - WASI clock interfaces
- `wit/world.wit` - VFS adapter world definition

## VFS Host Library (vfs-host)

The `vfs-host` library provides Host trait implementations for sharing VFS across multiple applications.

### Usage

```rust
use vfs_host::VfsHostState;
use wasmtime::{Engine, Store};

// Create shared VFS host
let engine = Engine::default();
let vfs_host = VfsHostState::new(&engine, "vfs-adapter.wasm")?;

// Create stores sharing the same VFS
let mut store1 = Store::new(&engine, vfs_host.clone_shared());
let mut store2 = Store::new(&engine, vfs_host.clone_shared());
```

### API

- `VfsHostState::new(engine, vfs_adapter_path)` - Create new VFS host
- `VfsHostState::clone_shared()` - Create host sharing same VFS
- All WASI filesystem Host trait methods implemented

See `vfs-host/README.md` and `examples/runtime-linker` for details.
