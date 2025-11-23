#!/bin/bash
# Compose application with VFS provider using wac

set -e

# Check if wac is installed
if ! command -v wac &> /dev/null; then
    echo "Error: wac is not installed"
    echo "Install with: cargo install wac-cli"
    exit 1
fi

echo "=== Composing Components with VFS Provider (using wac) ==="
echo

# Get VFS provider path
VFS_PROVIDER="../target/wasm32-wasip2/debug/vfs_provider.wasm"
if [ ! -f "$VFS_PROVIDER" ]; then
    echo "Error: VFS provider not found at $VFS_PROVIDER"
    echo "Build it with: cargo build -p vfs-provider --target wasm32-wasip2"
    exit 1
fi

# Compose Rust component
echo "Composing Rust component with wac plug..."
RUST_APP="component-rust/target/wasm32-wasip2/debug/component-rust.wasm"
if [ ! -f "$RUST_APP" ]; then
    echo "Error: Rust component not found. Run ./build-components.sh first"
    exit 1
fi

wac plug \
    --plug "$VFS_PROVIDER" \
    "$RUST_APP" \
    -o component-rust.composed.wasm

echo "✓ Composed Rust component: component-rust.composed.wasm"

# Compose C component (if available)
echo
C_APP="component-c/target/wasm32-wasip2/debug/component-c.wasm"
if [ -f "$C_APP" ]; then
    echo "Composing C component with wac plug..."
    wac plug \
        --plug "$VFS_PROVIDER" \
        "$C_APP" \
        -o component-c.composed.wasm

    echo "✓ Composed C component: component-c.composed.wasm"
else
    echo "⚠ C component not found (skipping, requires WASI SDK)"
fi

echo
echo "=== Composition completed ==="
echo "Run with: ./run-demo.sh"
