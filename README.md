# Halycon

An ephemeral in-memory filesystem implementation for WebAssembly, featuring transparent `std::fs` access through RPC communication.

## Overview

Halycon provides a shared virtual filesystem that multiple WASM applications can access transparently using standard Rust `std::fs` APIs. Applications don't need to know they're using a virtual filesystem. The RPC adapter handles all communication with the VFS server behind the scenes.

## Prerequisites

- Rust toolchain (1.75+)
- wasm32-wasip2 target: `rustup target add wasm32-wasip2`
- wasmtime CLI: `cargo install wasmtime-cli`

## Project Structure

```
halycon/
├── crates/
│   ├── core/fs-core/           # Core filesystem implementation (no_std compatible)
│   ├── adapters/
│   │   ├── vfs-adapter/        # WASI Component Model adapter
│   │   └── rpc-adapter/        # RPC communication adapter
│   ├── hosts/
│   │   ├── vfs-host/           # Host implementation for vfs-adapter
│   │   └── vfs-rpc-host/       # Host implementation for rpc-adapter
│   └── rpc/
│       ├── vfs-rpc-protocol/   # RPC protocol definitions
│       └── vfs-rpc-server/     # RPC server (WASM component)
├── examples/
│   ├── rpc/                    # RPC-based examples
│   │   ├── demo-writer/        # Writer application
│   │   ├── demo-reader/        # Reader application
│   │   ├── demo-std-fs/        # std::fs demonstration
│   │   └── demo-direct-rpc/    # Direct RPC communication demo
│   └── component-model/
│       ├── runtime-linker/     # Dynamic component loader
│       └── static/             # Static composition examples
├── wit/                        # WIT interface definitions
└── deprecated/                 # Legacy code (for reference)
```

## Building

### Build Native Crates

```bash
cargo build -p fs-core -p vfs-rpc-protocol -p vfs-host -p vfs-rpc-host
```

### Build WASM Components

```bash
cargo build --target wasm32-wasip2 \
  -p vfs-adapter \
  -p rpc-adapter \
  -p vfs-rpc-server \
  -p demo-writer \
  -p demo-reader \
  -p demo-std-fs \
  -p direct-rpc-demo
```

## Running Examples

### RPC Examples

#### Multi-Client Test (Writer + Reader)

This example demonstrates multiple applications sharing a single VFS instance:

```bash
# Use the provided script
./examples/rpc/run-multi-client-test.sh
```

Or manually:

```bash
# Build composed components
./examples/rpc/build-composed.sh

# Terminal 1: Start VFS RPC Server
wasmtime run -S inherit-network=y -S http ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm

# Terminal 2: Run Writer App (creates files)
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm

# Terminal 3: Run Reader App (reads files created by Writer)
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm
```

#### Direct RPC Demo

This example shows direct WASI socket communication with the VFS server:

```bash
# Use the provided script
./examples/rpc/run-direct-rpc-demo.sh
```

Or manually:

```bash
# Terminal 1: Start VFS RPC Server
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm

# Terminal 2: Run Direct RPC Demo
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/direct_rpc_demo.wasm
```

### Component Model Examples

#### Runtime Linker

The runtime-linker example demonstrates dynamic component loading with shared VFS:

```bash
# Build
cargo build -p runtime-linker
cargo build -p vfs-adapter --target wasm32-wasip2

# Run
./target/debug/runtime-linker ./target/wasm32-wasip2/debug/vfs_adapter.wasm <app.wasm>
```

## Testing

```bash
# Run fs-core tests
cargo test -p fs-core

# Run all tests
cargo test
```

## How It Works

1. VFS RPC Server: A WASM component running `fs-core` that listens on TCP port 9000. It manages an in-memory filesystem and handles requests from multiple clients.

2. RPC Adapter: A WASM component that implements WASI filesystem interfaces. When an application calls `std::fs::write()`, the rpc-adapter intercepts the call and forwards it to the VFS server via TCP.

3. Build-time Composition: Applications are composed with rpc-adapter using `wac plug` at build time, creating a self-contained WASM component that can run directly with wasmtime.

4. Application: Any WASM application using standard `std::fs` APIs. It doesn't need to know about the VFS; everything is transparent.

## Key Features

- Transparent VFS: Applications use standard `std::fs` APIs
- Multi-Client Support: Multiple applications can share the same VFS
- In-Memory: Fast, ephemeral storage (no persistence)
- WASI Compatible: Uses WASI Component Model
- no_std Core: The filesystem implementation is `no_std` compatible

## Development Setup

### Git Hooks (lefthook)

This project uses [lefthook](https://github.com/evilmartians/lefthook) for pre-commit checks.

```bash
lefthook install
```

Once installed, the following checks run automatically on `git commit`:
- `cargo fmt` - Auto-formats Rust code
- `cargo clippy` - Runs linter with warnings as errors

