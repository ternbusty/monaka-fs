# Examples

This directory contains example applications demonstrating two different approaches to using the VFS filesystem:

1. **Legacy Examples** - Direct library usage (single WASM module)
2. **Component Model Examples** - WASI composition (multiple components)

## Structure

### Component Model Examples
- `component-rust/` - Rust example using std::fs API
- `component-c/` - C example using standard C I/O functions
- `build-components.sh` - Build script for both examples
- `compose-demo.sh` - Composition script to connect apps with VFS provider
- `run-demo.sh` - Execution script for composed components

### Legacy Examples
- `c/` - C code using fs-wasm FFI directly
- `rust/` - Rust code using fs-wasm library directly

## Prerequisites

### Common Requirements

1. **Rust toolchain**:
   ```bash
   rustup update stable
   ```

2. **wasmtime** (for execution):
   ```bash
   cargo install wasmtime-cli
   ```

### Component Model Examples

1. **wasm32-wasip2 target**:
   ```bash
   rustup target add wasm32-wasip2
   ```

2. **wac** (WebAssembly Compositions, for component linking):
   ```bash
   cargo install wac-cli
   ```

3. **WASI SDK** (for C component):
   - Download from https://github.com/WebAssembly/wasi-sdk/releases
   - Extract to `~/wasi-sdk` or another location
   - Or use system clang if it supports wasm32-wasip2

### Legacy Examples

1. **wasm32-wasip1 target**:
   ```bash
   rustup target add wasm32-wasip1
   ```

2. **LLVM/clang with WASI support** (for C example):
   - macOS with Homebrew: `brew install llvm wasi-libc`
   - Linux: Install LLVM and wasi-libc from your package manager
   - Or download WASI SDK as above

## Quick Start

### Component Model Examples (Recommended)

1. **Build the VFS provider** (if not already built):
   ```bash
   cd ..
   cargo build -p vfs-provider --target wasm32-wasip2
   cd examples
   ```

2. **Build the example applications**:
   ```bash
   ./build-components.sh
   ```

3. **Compose with VFS provider**:
   ```bash
   ./compose-demo.sh
   ```

4. **Run the demos**:
   ```bash
   ./run-demo.sh
   ```

### Legacy Examples

From the repository root:

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

Both component-rust and component-c perform the following operations:

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

### Legacy Examples

Both c/ and rust/ examples perform the following operations:

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

## Architecture Comparison

### Component Model Approach

The component model examples use the WASI Preview 2 filesystem interface:

```
┌─────────────────────┐
│  Application        │
│  (Rust or C)        │
│  uses std::fs or    │
│  stdio.h            │
└──────────┬──────────┘
           │ imports wasi:filesystem
           │
┌──────────▼──────────┐
│  VFS Provider       │
│  Component          │
│  (in-memory FS)     │
└─────────────────────┘
```

The composition process:
1. Application imports `wasi:filesystem/types@0.2.6` and `wasi:filesystem/preopens@0.2.6`
2. VFS provider exports these interfaces
3. `wac plug` links them together by connecting imports to exports
4. The result is a self-contained component with in-memory filesystem

### Legacy Approach

The legacy examples link directly with the fs-wasm library:

```
┌─────────────────────┐
│  Application        │
│  (Rust or C)        │
│  uses fs_wasm FFI   │
│  or Rust API        │
├─────────────────────┤
│  fs-wasm Library    │
│  (FFI layer)        │
├─────────────────────┤
│  fs-core Library    │
│  (in-memory FS)     │
└─────────────────────┘
  Single WASM Module
```

The build process:
1. C or Rust code is compiled to WASM object files
2. Linked with fs-wasm and fs-core libraries
3. Produces a single WASM module
4. No component composition required

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
    --plug ../target/wasm32-wasip2/debug/vfs_provider.wasm \
    component-rust/target/wasm32-wasip2/debug/component-rust.wasm \
    -o component-rust.composed.wasm

# C component
wac plug \
    --plug ../target/wasm32-wasip2/debug/vfs_provider.wasm \
    component-c/target/wasm32-wasip2/debug/component-c.wasm \
    -o component-c.composed.wasm
```

This command:
- Takes the VFS provider as a `--plug` (the component that provides the missing imports)
- Takes the application component (the component that needs filesystem imports)
- Produces a composed component where all imports are satisfied

## Manual Execution

### Component Model Examples
```bash
# Run composed components
wasmtime run component-rust.composed.wasm
wasmtime run component-c.composed.wasm
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
- All tests should pass - if any fail, check the error output

### Legacy Examples

#### C compilation fails
- Ensure you have LLVM/clang with wasm32-wasip1 support
- For macOS with Homebrew: `brew install llvm wasi-libc`
- Check Makefile paths for WASI_SYSROOT and CLANG settings

#### Runtime errors
- Ensure wasmtime is installed: `cargo install wasmtime-cli`
- Legacy examples use wasm32-wasip1 target (not wasip2)
- Check that fs-core and fs-wasm are built: `cargo build`

## Next Steps

### For Component Model Development
- Explore the source code in `component-rust/src/main.rs` and `component-c/main.c`
- Learn how standard POSIX/C I/O APIs work with the VFS provider
- Create your own WASI Preview 2 applications
- Experiment with component composition using `wac`

### For Legacy Library Usage
- Explore the source code in `rust/src/main.rs` and `c/main.c`
- Learn how to use fs-wasm FFI directly from C
- Learn how to use fs-core library from Rust
- Build custom applications that need direct filesystem control

### General
- Read the [CLAUDE.md](../CLAUDE.md) for detailed architecture information
- Run tests: `cargo test` to understand the filesystem behavior
- Modify examples to test additional filesystem operations
