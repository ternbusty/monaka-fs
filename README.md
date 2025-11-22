# Halycon

Halycon is a virtual in-memory filesystem implemented in Rust and compiled to WebAssembly (WASM). It provides a C FFI layer for integration with C code.

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

### How to Build Examples

```bash
# Build all examples (C + Rust)
make examples

# Or build specific examples
make build-c-example          # C integration example
make build-rust-example       # Rust example
```

### How to Run Examples

```bash
# Run C integration example
make run-c-example

# Run Rust example
make run-rust-example

# Or run all examples
make run-example
```
