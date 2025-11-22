# Rust Example

## How to Build

```bash
# From repository root
make build-rust-example         # Debug build
make release-rust-example       # Release build

# Or manually
cargo build --target wasm32-wasip1
cargo build --target wasm32-wasip1 --release
```

## How to Run

```bash
# From repository root
make run-rust-example

# Or run directly
wasmtime run target/wasm32-wasip1/debug/rust-example.wasm
wasmtime run target/wasm32-wasip1/release/rust-example.wasm
```
