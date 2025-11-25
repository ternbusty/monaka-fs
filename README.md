# Halycon

Halycon is a virtual in-memory filesystem implemented in Rust and compiled to WebAssembly (WASM). It provides:
- **Component Model VFS Provider**: WASI Preview 2 filesystem implementation using the WebAssembly Component Model
- **Legacy C FFI layer**: For integration with C code (fs-wasm)

## Development Setup

### Git Hooks (lefthook)

This project uses [lefthook](https://github.com/evilmartians/lefthook) to run code formatting and linting checks before commits.

```bash
lefthook install
```

Once installed, the following checks run automatically on `git commit`.
- `cargo fmt` - Auto-formats Rust code
- `cargo clippy` - Runs linter with warnings as errors

## How to Build

```bash
# Build libraries only (fs-core, fs-wasm)
make build

# Or build libraries with release mode
make build-release
```

## How to Test

```bash
# Run all tests
cargo test

# Run tests for specific package
cargo test -p fs-core          # Core filesystem
cargo test -p fs-wasm          # FFI layer
```

**Note:** fs-wasm tests use the `serial_test` crate to ensure tests run sequentially, avoiding conflicts with the shared global filesystem state.

## Examples

This project provides two types of examples demonstrating different usage approaches:

### 1. Legacy Examples (Direct Library Usage)

These examples use fs-core and fs-wasm libraries directly through FFI or Rust APIs.

**Location**: `examples/c/` and `examples/rust/`

**Build and Run**:
```bash
# Build C example
make build-c-example
make run-c-example

# Build Rust example
make build-rust-example
make run-rust-example

# Or build all legacy examples
make examples
make run-example
```

**Architecture**: Application code links directly with fs-wasm library in a single WASM module.

### 2. Component Model Examples (WASI Composition)

These examples use standard POSIX/C I/O APIs composed with the VFS provider via WebAssembly Component Model.

**Location**: `examples/component-c/` and `examples/component-rust/`

**Build and Run**:
```bash
cd examples

# Build components
./build-components.sh

# Compose with VFS provider
./compose-demo.sh

# Run composed applications
./run-demo.sh
```

**Architecture**: Application components import WASI filesystem interfaces, which are satisfied by the VFS provider component through `wac plug` composition.

See [examples/README.md](examples/README.md) for detailed Component Model usage.

## Component Model VFS Provider

The VFS Provider is a WebAssembly Component Model implementation that exports WASI Preview 2 filesystem interfaces.

### Prerequisites

- Rust with `wasm32-wasip2` target
- [wac (WebAssembly Compositions)](https://github.com/bytecodealliance/wac) - for composing components
- [wasmtime](https://wasmtime.dev/) - for running components

```bash
# Install prerequisites
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

### Building the VFS Provider

```bash
# Build the VFS provider component
cargo build -p vfs-provider --target wasm32-wasip2

# Output: target/wasm32-wasip2/debug/vfs_provider.wasm
```

### Building and Running Sample Applications

The `examples/` directory contains sample applications that use the VFS provider.

#### Build Sample Application

```bash
# Build Rust sample component
cd examples/component-rust
cargo build --target wasm32-wasip2
```

#### Compose with VFS Provider

Use `wac plug` to compose the sample application with the VFS provider:

```bash
cd examples
./compose-demo.sh

# This creates: component-rust.composed.wasm
```

The composition script uses:
```bash
wac plug \
    --plug ../target/wasm32-wasip2/debug/vfs_provider.wasm \
    component-rust/target/wasm32-wasip2/debug/component-rust.wasm \
    -o component-rust.composed.wasm
```

#### Run Composed Component

```bash
./run-demo.sh

# Or run directly with wasmtime
wasmtime run component-rust.composed.wasm
```

### Current Status

The VFS provider successfully:
- Exports WASI filesystem/types@0.2.6 and filesystem/preopens@0.2.6 interfaces
- Provides root directory preopen
- Handles directory operations (mkdir, rmdir)
- Handles file deletion (unlink)
- Returns correct WASI error codes

Current limitations:
- File read/write operations return "Not supported" (requires stream API implementation)
- Stream APIs (`read_via_stream`, `write_via_stream`) are stubbed

### Architecture

```
┌─────────────────────────────────┐
│  Application Component          │
│  (imports wasi:filesystem)      │
└────────────┬────────────────────┘
             │ wac plug
             ▼
┌─────────────────────────────────┐
│  VFS Provider Component         │
│  (exports wasi:filesystem)      │
│  └─ uses fs-core internally     │
└─────────────────────────────────┘
             │ wasmtime
             ▼
┌─────────────────────────────────┐
│  WASI Runtime                   │
└─────────────────────────────────┘
```

### WIT Definitions

The VFS provider uses official WASI Preview 2 WIT definitions (v0.2.6):
- `wit/deps/filesystem/` - WASI filesystem interfaces
- `wit/deps/io/` - WASI I/O interfaces (streams, error, poll)
- `wit/deps/clocks/` - WASI clock interfaces
- `wit/world.wit` - VFS provider world definition
