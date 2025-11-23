#!/bin/bash
# Run composed component demos

set -e

# Check if wasmtime is installed
if ! command -v wasmtime &> /dev/null; then
    echo "Error: wasmtime is not installed"
    echo "Install with: cargo install wasmtime-cli"
    exit 1
fi

echo "=== Running Component Demos ==="
echo

# Run Rust component
if [ -f "component-rust.composed.wasm" ]; then
    echo "Running Rust component demo:"
    echo "----------------------------"
    wasmtime run component-rust.composed.wasm
    echo
else
    echo "Error: Rust component not found. Run ./compose-demo.sh first"
fi

# Run C component
if [ -f "component-c.composed.wasm" ]; then
    echo "Running C component demo:"
    echo "------------------------"
    wasmtime run component-c.composed.wasm
    echo
else
    echo "Error: C component not found. Run ./compose-demo.sh first"
fi

echo "=== Demos completed ==="
