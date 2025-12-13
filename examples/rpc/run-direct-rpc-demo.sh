#!/bin/bash
# Direct RPC Demo test script
# Demonstrates direct WASI socket communication with VFS RPC server
# (without using rpc-fs-runner)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$PROJECT_ROOT"

# Clean up any existing server
pkill -f vfs_rpc_server.wasm 2>/dev/null || true
sleep 1

echo "=== Building components ==="
cargo build -p vfs-rpc-server -p direct-rpc-demo --target wasm32-wasip2 2>&1 | tail -1
echo "Build complete"

echo ""
echo "=== Starting VFS RPC Server ==="
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm > /dev/null 2>&1 &
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
echo "=== Running Direct RPC Demo ==="
echo "(Using WASI sockets directly, no rpc-fs-runner needed)"
echo ""
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/direct_rpc_demo.wasm 2>&1

echo ""
echo "=== Demo completed ==="
