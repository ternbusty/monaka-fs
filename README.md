# Halycon

Halycon is a virtual in-memory filesystem implemented in Rust and compiled to WebAssembly (WASM). It provides:
- **Component Model VFS Adapter**: WASI Preview 2 filesystem implementation using the WebAssembly Component Model
- **Runtime Dynamic Linking**: Modular component composition at runtime (recommended)
- **Legacy C FFI layer**: For integration with C code (fs-wasm)

## Quick Start

```bash
make demo  # Run the recommended dynamic linking demo
```

**Works from any directory:**
```bash
# From repository root:
make demo

# Or from examples directory:
cd examples
make demo
```

See all available commands:
```bash
make help
```

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

This project provides three types of examples demonstrating different usage approaches:

### 1. Component Model - Dynamic Linking (Recommended)

Runtime dynamic linking allows loading and composing components at runtime for maximum modularity.

**Quick Start**:
```bash
make demo
# or
make demo-dynamic
```

**What it demonstrates**:
- Loading VFS adapter and application components separately
- Runtime composition using `wac plug` (~14ms overhead)
- Performance comparison with static composition
- Independent component updates without rebuilding
- All 20 filesystem tests passing with in-memory VFS

**Architecture**: Components are loaded separately and composed at runtime, allowing swap of VFS implementations.

**Advantages**:
- ✓ Maximum modularity and flexibility
- ✓ Swap VFS implementations at runtime
- ✓ Independent component updates
- ✓ Minimal runtime overhead (< 0.1% size, ~14ms)

### 2. Component Model - Static Composition

Build-time linking produces a single composed WASM file for simpler deployment.

**Quick Start**:
```bash
make demo-static
# or individually:
make run-static-rust
make run-static-c
```

**Build Only**:
```bash
make compose-static-rust  # Produces examples/component-rust.composed.wasm
make compose-static-c     # Produces examples/component-c.composed.wasm
```

**Architecture**: Application components are linked with VFS adapter at build time using `wac plug`, producing a single `.composed.wasm` file.

**Advantages**:
- ✓ Single file distribution
- ✓ No runtime composition overhead
- ✓ Simpler deployment

### 3. Legacy Examples (Direct Library Usage)

Direct FFI/library usage without Component Model for maximum control.

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

**Advantages**:
- ✓ Direct library access
- ✓ No component model overhead
- ✓ Simple build process

---

See [examples/README.md](examples/README.md) for detailed usage and comparison.

## Component Model VFS Adapter

The VFS Adapter is a WebAssembly Component Model implementation that exports WASI Preview 2 filesystem interfaces.

### Prerequisites

- Rust with `wasm32-wasip2` target
- [wac (WebAssembly Compositions)](https://github.com/bytecodealliance/wac) - for composing components
- [wasmtime](https://wasmtime.dev/) - for running components

```bash
# Install prerequisites
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

### Building the VFS Adapter

```bash
# Build the VFS adapter component
cargo build -p vfs-adapter --target wasm32-wasip2

# Output: target/wasm32-wasip2/debug/vfs_adapter.wasm
```

### Quick Start with Dynamic Linking (Recommended)

The fastest way to see it in action:

```bash
make demo
```

This demonstrates:
- Loading VFS adapter and application components separately
- Runtime composition using `wac plug`
- Performance comparison with static composition
- All filesystem operations working with in-memory VFS

### Alternative: Static Composition

For build-time composition:

```bash
make demo-static
# or individually:
make run-static-rust
make run-static-c
```

The composition script uses:
```bash
wac plug \
    --plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
    component-rust/target/wasm32-wasip2/debug/component-rust.wasm \
    -o component-rust.composed.wasm
```

### Current Status

The VFS adapter **fully implements** WASI Preview 2 filesystem interfaces:
- ✓ Exports WASI filesystem/types@0.2.6 and filesystem/preopens@0.2.6 interfaces
- ✓ Provides root directory preopen
- ✓ Full file operations: read, write, append, seek, truncate
- ✓ Full directory operations: mkdir, rmdir, readdir, stat
- ✓ Stream APIs fully implemented (`read_via_stream`, `write_via_stream`)
- ✓ All WASI error codes correctly mapped
- ✓ Complete metadata support (size, timestamps, file types)
- ✓ All tests passing (20/20 in component-rust, 20/20 in component-c)

**Status**: Production-ready for in-memory filesystem use cases

### Architecture

#### Dynamic Linking (Recommended)

```
┌─────────────────────┐     ┌─────────────────────┐
│  vfs_adapter.wasm   │     │  application.wasm   │
│  (4.24 MB)          │     │  (2.46 MB)          │
└──────────┬──────────┘     └──────────┬──────────┘
           │                           │
           └──────────┬────────────────┘
                      │ wac plug (runtime)
                      ▼
           ┌──────────────────────┐
           │  composed.wasm       │
           │  (6.70 MB)           │
           └──────────────────────┘
                      │ wasmtime
                      ▼
           ┌──────────────────────┐
           │  WASI Runtime        │
           └──────────────────────┘
```

#### Static Composition (Alternative)

```
┌─────────────────────────────────┐
│  Application Component          │
│  (imports wasi:filesystem)      │
└────────────┬────────────────────┘
             │ wac plug (build time)
             ▼
┌─────────────────────────────────┐
│  VFS Adapter Component          │
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

The VFS adapter uses official WASI Preview 2 WIT definitions (v0.2.6):
- `wit/deps/filesystem/` - WASI filesystem interfaces
- `wit/deps/io/` - WASI I/O interfaces (streams, error, poll)
- `wit/deps/clocks/` - WASI clock interfaces
- `wit/world.wit` - VFS adapter world definition
