#!/bin/bash
# Build Component Model samples

set -e

echo "=== Building Component Model Samples ==="
echo

# Build Rust sample
echo "Building Rust component..."
cd component-rust
cargo build --target wasm32-wasip2
echo "✓ Rust component built: target/wasm32-wasip2/debug/component-rust.wasm"
cd ..

# Build C sample
echo
echo "Building C component..."
cd component-c
cargo build --target wasm32-wasip2
echo "✓ C component built: target/wasm32-wasip2/debug/component-c.wasm"
cd ..

echo
echo "=== Build completed ==="
