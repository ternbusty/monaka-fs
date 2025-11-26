# Examples

This directory contains examples demonstrating different approaches to using the VFS filesystem.

## Structure

### Runtime Dynamic Linking
- `runtime-linker/` - Runtime component loading and composition

### Component Model Examples (Static Composition)
- `component-rust/` - Rust example using std::fs API
- `component-c/` - C example using standard C I/O functions

### RPC Examples
- `vfs-rpc-server/` (in parent) - TCP server exposing VFS on port 9000
- `vfs-demo-app1/` - Writer application
- `vfs-demo-app2/` - Reader application
- `wasm-runner/` - Host program with network permissions

### Legacy Examples
- `c/` - C code using fs-wasm FFI directly
- `rust/` - Rust code using fs-wasm library directly

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
# From repository root or examples directory:
make demo
```

### Component Model - Static Composition

```bash
make demo-static

# Or individually:
make run-static-rust
make run-static-c
```

### RPC Approach

```bash
# Terminal 1: Start VFS RPC Server
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

### Legacy Examples

```bash
# C example
make build-c-example
make run-c-example

# Rust example
make build-rust-example
make run-rust-example
```

Or directly:

```bash
# C example
cd examples/c
make
wasmtime run ../../target/wasm32-wasip1/debug/c_example.wasm

# Rust example
cd examples/rust
cargo build --target wasm32-wasip1
wasmtime run target/wasm32-wasip1/debug/rust-example.wasm
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

**vfs-demo-app1** (Writer):
- Connects to VFS RPC server on localhost:9000
- Creates `/shared` directory
- Creates and writes to `/shared/message.txt`
- Demonstrates file creation over network protocol

**vfs-demo-app2** (Reader):
- Connects to same VFS RPC server
- Opens `/shared/message.txt` created by App1
- Reads file content
- Gets file metadata
- Demonstrates file sharing between separate WASM processes

### Legacy Examples

Both c/ and rust/ examples perform:

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
cd component-rust
cargo build --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/component-rust.wasm`

#### C Component
```bash
cd component-c
cargo build --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/component-c.wasm`

### RPC Components

```bash
# Build wasm-runner host
cargo build -p wasm-runner

# Build VFS RPC server
cargo build -p vfs-rpc-server --target wasm32-wasip2

# Build demo applications
cargo build -p vfs-demo-app1 --target wasm32-wasip2
cargo build -p vfs-demo-app2 --target wasm32-wasip2
```

### Legacy Examples

#### C Example
```bash
cd c
make
```

Output: `../../target/wasm32-wasip1/debug/c_example.wasm`

Or from repository root:
```bash
make build-c-example
```

#### Rust Example
```bash
cd rust
cargo build --target wasm32-wasip1
```

Output: `target/wasm32-wasip1/debug/rust-example.wasm`

Or from repository root:
```bash
make build-rust-example
```

## Manual Composition (Component Model Only)

Using `wac plug` to connect the VFS provider to your application:

```bash
# Rust component
wac plug \
    --plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
    component-rust/target/wasm32-wasip2/debug/component-rust.wasm \
    -o component-rust.composed.wasm

# C component
wac plug \
    --plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
    component-c/target/wasm32-wasip2/debug/component-c.wasm \
    -o component-c.composed.wasm
```

## Manual Execution

### Component Model Examples
```bash
# Run composed components
wasmtime run component-rust.composed.wasm
wasmtime run component-c.composed.wasm
```

### RPC Examples
```bash
# Run with wasm-runner host
./target/debug/wasm-runner target/wasm32-wasip2/debug/vfs_rpc_server.wasm
./target/debug/wasm-runner target/wasm32-wasip2/debug/vfs_demo_app1.wasm
./target/debug/wasm-runner target/wasm32-wasip2/debug/vfs_demo_app2.wasm
```

### Legacy Examples
```bash
# Run standalone WASM modules
wasmtime run ../target/wasm32-wasip1/debug/c_example.wasm
wasmtime run rust/target/wasm32-wasip1/debug/rust-example.wasm
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
- Ensure VFS RPC server is running first
- Check that port 9000 is not in use: `lsof -i :9000`
- Verify wasm-runner is built: `cargo build -p wasm-runner`

#### Network permission errors
- wasm-runner host enables network permissions via `inherit_network()`
- Standard wasmtime will not work; must use wasm-runner

### Legacy Examples

#### C compilation fails
- Ensure you have LLVM/clang with wasm32-wasip1 support
- For macOS with Homebrew: `brew install llvm wasi-libc`
- Check Makefile paths for WASI_SYSROOT and CLANG settings

#### Runtime errors
- Ensure wasmtime is installed: `cargo install wasmtime-cli`
- Legacy examples use wasm32-wasip1 target (not wasip2)
- Check that fs-core and fs-wasm are built: `cargo build`
