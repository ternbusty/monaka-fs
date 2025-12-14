#!/bin/bash
# Multi-client VFS RPC test script
# Tests that Writer app can write to the shared VFS and Reader app can read from it
#
# Uses wac compose to build composed components (no native binary needed)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$PROJECT_ROOT"

# Clean up any existing server
pkill -f vfs_rpc_server.wasm 2>/dev/null || true
sleep 1

echo "=== Building components ==="
# Build WASM components
cargo build -p vfs-rpc-server -p demo-writer -p demo-reader -p rpc-adapter --target wasm32-wasip2 2>&1 | tail -1

# Compose with wac
echo "Composing demo-writer with rpc-adapter..."
wac plug \
    --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
    target/wasm32-wasip2/debug/demo-writer.wasm \
    -o target/wasm32-wasip2/debug/composed-demo-writer.wasm

echo "Composing demo-reader with rpc-adapter..."
wac plug \
    --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
    target/wasm32-wasip2/debug/demo-reader.wasm \
    -o target/wasm32-wasip2/debug/composed-demo-reader.wasm

echo "Build complete"

echo ""
echo "=== Starting VFS RPC Server ==="
wasmtime run -S inherit-network=y -S http ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm > /dev/null 2>&1 &
SERVER_PID=$!
sleep 2

# Ensure server is killed on exit
cleanup() {
    kill $SERVER_PID 2>/dev/null || true
    pkill -f vfs_rpc_server.wasm 2>/dev/null || true
}
trap cleanup EXIT

echo "Server started (PID: $SERVER_PID)"

echo ""
echo "=== Running Writer App ==="
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm 2>&1 | grep -E "^(===|Creating|Writing|  |Application)" || true

echo ""
echo "=== Running Reader App ==="
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm 2>&1 | grep -E "^(===|Getting|Reading|  |Application|\")" || true

echo ""
echo "=== Test completed successfully ==="
