# Rust Example (Legacy/Deprecated)

Note: This is a legacy example using the deprecated fs-wasm library with wasm32-wasip1 target. For the current approach, see the examples in `/examples/` which use wasm32-wasip2.

## How to Build

```bash
# From repository root
make build-legacy-rust           # Debug build
make build-legacy-rust-release   # Release build

# Or manually
cargo build --manifest-path deprecated/legacy-examples/rust/Cargo.toml --target-dir target --target wasm32-wasip1
cargo build --manifest-path deprecated/legacy-examples/rust/Cargo.toml --target-dir target --target wasm32-wasip1 --release
```

## How to Run

```bash
# From repository root
make run-legacy-rust

# Or run directly
wasmtime run target/wasm32-wasip1/debug/rust-example.wasm
wasmtime run target/wasm32-wasip1/release/rust-example.wasm
```
