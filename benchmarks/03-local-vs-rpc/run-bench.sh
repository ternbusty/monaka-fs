#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

echo "=== Building benchmark app ==="
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2

# Standalone workspace builds to its own target directory
BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-local-vs-rpc.wasm"

echo ""
echo "=== Building adapters ==="
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter -p rpc-adapter

VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"
RPC_ADAPTER="$BUILD_DIR/rpc_adapter.wasm"

echo ""
echo "=== Building RPC server ==="
cargo build --release --target wasm32-wasip2 -p vfs-rpc-server

RPC_SERVER="$BUILD_DIR/vfs_rpc_server.wasm"

echo ""
echo "=== Composing WASM components ==="

LOCAL_COMPOSED="$SCRIPT_DIR/bench-local.wasm"
RPC_COMPOSED="$SCRIPT_DIR/bench-rpc.wasm"

wac plug --plug "$VFS_ADAPTER" "$BENCH_WASM" -o "$LOCAL_COMPOSED"
wac plug --plug "$RPC_ADAPTER" "$BENCH_WASM" -o "$RPC_COMPOSED"

echo ""
echo "=========================================="
echo "  Running: Local VFS (wac plug)"
echo "=========================================="

LOCAL_RESULTS=$(wasmtime run "$LOCAL_COMPOSED" 2>&1)
echo "$LOCAL_RESULTS"

echo ""
echo "=========================================="
echo "  Running: RPC VFS"
echo "=========================================="

# Start RPC server in background
echo "Starting vfs-rpc-server..."
wasmtime run -S inherit-network=y -S http "$RPC_SERVER" &
RPC_PID=$!

# Wait for server to start
sleep 1

# Check if server is running
if ! kill -0 $RPC_PID 2>/dev/null; then
    echo "[ERROR] Failed to start RPC server"
    exit 1
fi

# Cleanup function
cleanup() {
    if [ -n "$RPC_PID" ] && kill -0 $RPC_PID 2>/dev/null; then
        echo "Stopping RPC server..."
        kill $RPC_PID 2>/dev/null || true
        wait $RPC_PID 2>/dev/null || true
    fi
    rm -f "$LOCAL_COMPOSED" "$RPC_COMPOSED"
}
trap cleanup EXIT

RPC_RESULTS=$(wasmtime run -S inherit-network=y "$RPC_COMPOSED" 2>&1)
echo "$RPC_RESULTS"

echo ""
echo "=========================================="
echo "  Comparison Summary"
echo "=========================================="

echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""
echo "--- Local VFS (wac plug) ---"
echo "$LOCAL_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- RPC VFS ---"
echo "$RPC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'

echo ""
echo "=== Benchmark Complete ==="
