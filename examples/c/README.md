# C Example

This directory demonstrates C code using the Rust-based ephemeral filesystem.

## How to Build

```bash
# From repository root
make build-c-example
```

## How to Run

```bash
# From repository root
make run-c-example

# Or run directly
wasmtime run ./target/wasm32-wasip1/debug/c_example.wasm
```
