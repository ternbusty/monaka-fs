# Halycon

Halycon is a virtual in-memory filesystem implemented in Rust and compiled to WebAssembly (WASM). It provides:
- **Component Model VFS Adapter**: WASI Preview 2 filesystem implementation using the WebAssembly Component Model
- **VFS Host Traits**: Rust library for sharing VFS across multiple applications at runtime
- **Runtime Dynamic Linking**: Modular component composition at runtime (recommended)
- **Legacy C FFI layer**: For integration with C code (fs-wasm)

## Project Structure

### Distributable Libraries

- **fs-core** - Core in-memory filesystem implementation (no_std compatible)
- **vfs-adapter** - WebAssembly Component Model VFS adapter (WASI Preview 2)
- **vfs-host** - Host trait implementation for sharing VFS across multiple applications
- **fs-wasm** - Legacy C FFI layer (for non-Component Model use cases)

### When to Use Each Library

**For Component Model applications:**
1. **Simple single-app use**: Use `vfs-adapter` with static or runtime composition (`wac plug`)
2. **Multiple apps sharing VFS**: Use `vfs-host` library to implement custom host with shared VFS instance
3. **Reference implementation**: See `examples/runtime-linker` for complete example

**For legacy applications:**
- Use `fs-wasm` with direct FFI calls (see `examples/c` and `examples/rust`)

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

## VFS Host Library (vfs-host)

The `vfs-host` library provides Host trait implementations that enable **multiple applications to share a single VFS instance at runtime**. This is the recommended approach for complex use cases requiring shared filesystem state.

### Key Features

- **Shared VFS State**: Multiple applications access the same VFS instance concurrently
- **Thread-Safe**: Uses `Arc<Mutex<>>` for safe concurrent access
- **State Persistence**: VFS state persists as long as any application references it
- **Complete WASI Implementation**: All 33 WASI filesystem Host trait methods implemented
  - 26 real implementations (file I/O, directories, metadata, stream API, etc.)
  - 7 stub implementations (advisory hints, sync operations)
- **Full Stream API Support**: Complete implementation of `read_via_stream`, `write_via_stream`, `append_via_stream`
- **Zero-Copy Resource Mapping**: Efficient descriptor and stream resource management

### When to Use vfs-host

Use the `vfs-host` library when you need:
1. Multiple applications sharing the same filesystem
2. Persistent filesystem state across application lifecycles
3. Runtime flexibility in VFS provider selection
4. Custom host implementations with shared resources

### Usage Example

```rust
use vfs_host::VfsHostState;
use wasmtime::{Engine, Store};

// Create shared VFS host
let engine = Engine::default();
let vfs_host = VfsHostState::new(&engine, "vfs-adapter.wasm")?;

// Create first application store
let mut store1 = Store::new(&engine, vfs_host.clone_shared());

// Create second application store sharing the same VFS
let mut store2 = Store::new(&engine, vfs_host.clone_shared());

// Both applications see each other's changes immediately!
// App1 creates a file -> App2 can read it
// App2 modifies a file -> App1 sees the changes
```

### Architecture with vfs-host

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ Application1 │    │ Application2 │    │ Application3 │
└──────┬───────┘    └──────┬───────┘    └──────┬───────┘
       │                   │                   │
       │    VfsHostState (clone_shared())      │
       └───────────────────┼───────────────────┘
                           │
                  Arc<Mutex<SharedVfsCore>>
                           │
                   ┌───────┴───────┐
                   │  VFS Adapter  │
                   │  (Component)  │
                   └───────────────┘
```

### Complete Example

See `examples/runtime-linker` for a complete working example demonstrating:
- Creating shared VFS host state
- Multiple applications accessing the same VFS
- State persistence after application termination
- Concurrent filesystem operations

```bash
# Run the complete demo
cd examples/runtime-linker
cargo run
```

### API Reference

**Core Types:**
- `VfsHostState` - Host context implementing WASI Host traits
- `SharedVfsCore` - Shared VFS state wrapped in Arc<Mutex<>>
- `VfsStoreData` - Store data for VFS adapter instance

**Main Methods:**
- `VfsHostState::new(engine, vfs_adapter_path)` - Create new VFS host
- `VfsHostState::clone_shared()` - Create new host sharing the same VFS
- All WASI filesystem Host trait methods (automatically implemented)

### Comparison: wac plug vs vfs-host

| Approach | Use Case | Shared State | Complexity |
|----------|----------|--------------|------------|
| `wac plug` (static/runtime) | Single application | No | Low |
| `vfs-host` library | Multiple applications | Yes | Medium |

**Choose `wac plug` when:**
- Single application needs VFS
- No shared state required
- Simpler deployment model

**Choose `vfs-host` when:**
- Multiple applications need shared VFS
- State must persist across app lifecycles
- Runtime flexibility is important
