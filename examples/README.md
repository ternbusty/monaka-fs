# Component Model Examples

This directory contains example applications that demonstrate the use of the VFS provider component.

## Structure

- `component-rust/` - Rust example using std::fs API
- `component-c/` - C example using standard C I/O functions
- `build-components.sh` - Build script for both examples
- `compose-demo.sh` - Composition script to connect apps with VFS provider
- `run-demo.sh` - Execution script for composed components

## Prerequisites

1. **Rust toolchain with wasm32-wasip2 target**:
   ```bash
   rustup target add wasm32-wasip2
   ```

2. **wac** (WebAssembly Compositions, for component linking):
   ```bash
   cargo install wac-cli
   ```

3. **wasmtime** (for execution):
   ```bash
   cargo install wasmtime-cli
   ```

4. **WASI SDK** (for C example, optional):
   - Download from https://github.com/WebAssembly/wasi-sdk
   - Or use system clang if it supports wasm32-wasip2

## Quick Start

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

## What the Examples Demonstrate

Both examples perform the following operations:

### Test 1: Basic File Operations
- Create and write to a file
- Read file contents
- Append to file
- Delete file

### Test 2: Directory Operations
- Create directories
- Create files in directories
- List directory contents
- Remove directories

### Test 3: Metadata Operations
- Get file metadata (size, type, etc.)
- Truncate files

### Test 4: Error Handling
- Access non-existent files
- Handle duplicate directory creation
- Read directory as file

## Architecture

The examples use the WASI Preview 2 filesystem interface:

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

## Individual Component Builds

### Rust Component
```bash
cd component-rust
cargo build --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/component-rust.wasm`

### C Component
```bash
cd component-c
cargo build --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/component-c.wasm`

## Manual Composition

Using `wac plug` to connect the VFS provider to your application:

```bash
wac plug \
    --plug ../target/wasm32-wasip2/debug/vfs_provider.wasm \
    component-rust/target/wasm32-wasip2/debug/component-rust.wasm \
    -o component-rust.composed.wasm
```

This command:
- Takes the VFS provider as a `--plug` (the component that provides the missing imports)
- Takes the application component (the component that needs filesystem imports)
- Produces a composed component where all imports are satisfied

## Manual Execution

```bash
wasmtime run component-rust.composed.wasm
```

## Troubleshooting

### C compilation fails
- Ensure you have WASI SDK or clang with wasm32-wasip2 support
- Set `CC` environment variable to point to your WASI clang:
  ```bash
  export CC=/path/to/wasi-sdk/bin/clang
  ```

### Composition fails
- Ensure VFS provider is built for wasm32-wasip2
- Check that wac-cli is installed: `cargo install wac-cli`
- Verify version compatibility: Both components should use WASI 0.2.6

### Runtime errors
- Check wasmtime version (should support WASI Preview 2): `wasmtime --version`
- Ensure the composed component includes the VFS provider
- Some operations may return "Not supported" - this is expected for unimplemented stream APIs

## Next Steps

- Explore the source code in `component-rust/src/main.rs` and `component-c/main.c`
- Modify the examples to test additional filesystem operations
- Create your own applications using WASI filesystem
