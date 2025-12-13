# Examples

This directory contains examples demonstrating different approaches to using the VFS filesystem.

## Structure

### Component Model Examples

#### Dynamic Linking
- `component-model/runtime-linker/` - Runtime component loading and composition

#### Static Composition
- `component-model/static/rust/` - Rust example using std::fs API
- `component-model/static/c/` - C example using standard C I/O functions

### RPC Examples
- `rpc/demo-writer/` - Writer application
- `rpc/demo-reader/` - Reader application
- `rpc/demo-std-fs/` - Standard filesystem demo
- `rpc/demo-direct-rpc/` - Direct RPC demo

### Legacy Examples (Deprecated)
- `../deprecated/legacy-examples/c/` - C code using fs-wasm FFI directly
- `../deprecated/legacy-examples/rust/` - Rust code using fs-wasm library directly

## Prerequisites

### Common Requirements

```bash
rustup update stable
cargo install wasmtime-cli
```

### Component Model Examples

```bash
rustup target add wasm32-wasip2
cargo install wac-cli
```

For C component, download [WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases) and extract to `~/wasi-sdk`.

### Legacy Examples

```bash
rustup target add wasm32-wasip1

# macOS:
brew install llvm wasi-libc

# Or download WASI SDK
```

## Quick Start

### Runtime Dynamic Linking

```bash
# From repository root:
make demo-component-model-dynamic
```

### Component Model - Static Composition

```bash
make demo-component-model-static

# Or individually:
make run-component-model-static-rust
make run-component-model-static-c
```

### RPC Approach

```bash
# Build all RPC components
make build-rpc-all

# Terminal 1: Start VFS RPC Server
make start-rpc-server

# Terminal 2: Run demos
make run-rpc-demo-writer
make run-rpc-demo-reader
make run-rpc-demo-std-fs

# Stop server when done
make stop-rpc-server
```

### Legacy Examples

```bash
# C example
make build-legacy-c
make run-legacy-c

# Rust example
make build-legacy-rust
make run-legacy-rust

# Release builds
make build-legacy-c-release
make build-legacy-rust-release
```

## What the Examples Demonstrate

### Component Model Examples

Both component-rust and component-c perform:

#### Test 1: Basic File Operations
- Create and write to a file
- Read file contents
- Append to file
- Delete file

#### Test 2: Directory Operations
- Create directories
- Create files in directories
- List directory contents
- Remove directories

#### Test 3: Metadata Operations
- Get file metadata (size, type, etc.)
- Truncate files

#### Test 4: Error Handling
- Access non-existent files
- Handle duplicate directory creation
- Read directory as file

### RPC Examples

demo-writer (Writer):
- Connects to VFS RPC server on localhost:9000
- Creates `/shared` directory
- Creates and writes to `/shared/message.txt`
- Demonstrates file creation over network protocol

demo-reader (Reader):
- Connects to same VFS RPC server
- Opens `/shared/message.txt` created by demo-writer
- Reads file content
- Gets file metadata
- Demonstrates file sharing between separate WASM processes

### Legacy Examples (Deprecated)

Both deprecated/legacy-examples/c/ and deprecated/legacy-examples/rust/ examples perform:

#### Test 1: Basic File Operations
- Create files and write content
- Read file contents
- Verify data integrity

#### Test 2: Multiple Files
- Create multiple files simultaneously
- Verify each file independently

#### Test 3: Large File Handling
- Write and read 16KB files
- Test buffer management

#### Test 4: Sparse Files
- Write at different file positions
- Seek operations
- Verify sparse regions

#### Test 5: O_APPEND Operations
- Append mode file operations
- Multiple append operations

#### Test 6: O_TRUNC Operations
- File truncation on open
- Overwrite existing content

## Individual Builds

### Component Model Examples

#### Rust Component
```bash
make build-component-model-rust
# Or manually:
cd component-model/static/rust
cargo build --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/component-rust.wasm`

#### C Component
```bash
make build-component-model-c
# Or manually:
cd component-model/static/c
cargo build --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/component-c.wasm`

### RPC Components

```bash
# Build all
make build-rpc-all

# Or individually:
make build-rpc-server   # VFS RPC server
make build-rpc-runner   # rpc-fs-runner host
make build-rpc-demos    # demo applications
```

### Legacy Examples (Deprecated)

```bash
# Build
make build-legacy-c
make build-legacy-rust

# Release builds
make build-legacy-c-release
make build-legacy-rust-release
```

## Manual Composition (Component Model Only)

Using `wac plug` to connect the VFS provider to your application:

```bash
# Rust component
make compose-component-model-rust

# C component
make compose-component-model-c
```

## Manual Execution

### Component Model Examples
```bash
# Run composed components
make run-component-model-static-rust
make run-component-model-static-c
```

### RPC Examples
```bash
# Start VFS RPC server (Terminal 1)
make start-rpc-server

# Run demo applications (separate terminals)
make run-rpc-demo-writer
make run-rpc-demo-reader
make run-rpc-demo-std-fs

# Stop server
make stop-rpc-server
```

### Legacy Examples (Deprecated)
```bash
make run-legacy-c
make run-legacy-rust
```

## Troubleshooting

### Component Model Examples

#### C compilation fails
- Ensure you have WASI SDK or clang with wasm32-wasip2 support
- Download WASI SDK: https://github.com/WebAssembly/wasi-sdk/releases
- Install to `~/wasi-sdk` or set `CC` environment variable:
  ```bash
  export CC=/path/to/wasi-sdk/bin/clang
  ```

#### Composition fails
- Ensure VFS provider is built for wasm32-wasip2
- Check that wac-cli is installed: `cargo install wac-cli`
- Verify version compatibility: Both components should use WASI 0.2.6

#### Runtime errors
- Check wasmtime version (should support WASI Preview 2): `wasmtime --version`
- Ensure the composed component includes the VFS provider

### RPC Examples

#### Connection refused
- Ensure VFS RPC server is running first: `make start-rpc-server`
- Check that port 9000 is not in use: `lsof -i :9000`
- Verify rpc-fs-runner is built: `make build-rpc-runner`

#### Network permission errors
- rpc-fs-runner host enables network permissions via `inherit_network()`
- For server, use `wasmtime run -S inherit-network=y`

### Legacy Examples (Deprecated)

#### C compilation fails
- Ensure you have LLVM/clang with wasm32-wasip1 support
- For macOS with Homebrew: `brew install llvm wasi-libc`
- Check Makefile paths for WASI_SYSROOT and CLANG settings

#### Runtime errors
- Ensure wasmtime is installed: `cargo install wasmtime-cli`
- Legacy examples use wasm32-wasip1 target (not wasip2)
- Check that fs-core and fs-wasm are built: `cargo build`
