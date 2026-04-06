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
│   └── rpc/
│       ├── vfs-rpc-protocol/   # RPC protocol definitions
│       └── vfs-rpc-server/     # RPC server (WASM component)
├── examples/
│   ├── static-composition/     # Build-time composition with vfs-adapter
│   ├── host-trait/             # Runtime dynamic linking with vfs-host
│   └── rpc-server/             # TCP-based sharing via vfs-rpc-server
├── wit/                        # WIT interface definitions
└── deprecated/                 # Legacy code (for reference)
```

## Building

### Build Native Crates

```bash
cargo build -p fs-core -p vfs-rpc-protocol -p vfs-host
```

### Build WASM Components

```bash
cargo build --target wasm32-wasip2 \
  -p vfs-adapter \
  -p rpc-adapter \
  -p vfs-rpc-server \
  -p demo-writer \
  -p demo-reader \
  -p demo-fs-operations
```

## Running Examples

See the [`examples/`](./examples/) directory for detailed instructions on each deployment method:

- **[Static Composition](./examples/static-composition/)** — Build-time composition with `wac plug` + `vfs-adapter`
- **[Host Trait](./examples/host-trait/)** — Runtime dynamic linking with `vfs-host`
- **[RPC Server](./examples/rpc-server/)** — TCP-based sharing via `vfs-rpc-server`

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

