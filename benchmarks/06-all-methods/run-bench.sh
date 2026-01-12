#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

echo "=============================================="
echo "  Benchmark: All VFS Methods Comparison"
echo "=============================================="
echo ""
echo "Methods:"
echo "  1. Static Compose (wac plug + vfs-adapter)"
echo "  2. Host Trait (wasmtime + vfs-host)"
echo "  3. RPC (rpc-adapter + vfs-rpc-server)"
echo ""

# =============================================================================
# Build
# =============================================================================

echo "=== Building benchmark app ==="
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2
BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-all-methods.wasm"

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
echo "=== Building bench-runner (for host trait method) ==="
cd "$SCRIPT_DIR/bench-runner"
cargo build --release

echo ""
echo "=== Composing WASM components ==="

STATIC_COMPOSED="$SCRIPT_DIR/bench-static.wasm"
RPC_COMPOSED="$SCRIPT_DIR/bench-rpc.wasm"

wac plug --plug "$VFS_ADAPTER" "$BENCH_WASM" -o "$STATIC_COMPOSED"
wac plug --plug "$RPC_ADAPTER" "$BENCH_WASM" -o "$RPC_COMPOSED"

# =============================================================================
# Run Benchmarks
# =============================================================================

echo ""
echo "=============================================="
echo "  Method 1: Static Compose (wac plug)"
echo "=============================================="
echo ""

STATIC_RESULTS=$(wasmtime run "$STATIC_COMPOSED" 2>&1)
echo "$STATIC_RESULTS"

echo ""
echo "=============================================="
echo "  Method 2: Host Trait (vfs-host)"
echo "=============================================="
echo ""

HOST_RESULTS=$("$SCRIPT_DIR/bench-runner/target/release/bench-runner" "$BENCH_WASM" 2>&1)
echo "$HOST_RESULTS"

echo ""
echo "=============================================="
echo "  Method 3: RPC (vfs-rpc-server)"
echo "=============================================="
echo ""

# Start RPC server in background
echo "Starting vfs-rpc-server..."
wasmtime run -S inherit-network=y -S http "$RPC_SERVER" > /tmp/vfs-server-bench.log 2>&1 &
RPC_PID=$!

# Wait for server to start
sleep 2

# Check if server is running
if ! kill -0 $RPC_PID 2>/dev/null; then
    echo "[ERROR] Failed to start RPC server"
    cat /tmp/vfs-server-bench.log
    exit 1
fi

# Cleanup function
cleanup() {
    if [ -n "$RPC_PID" ] && kill -0 $RPC_PID 2>/dev/null; then
        echo ""
        echo "Stopping RPC server..."
        kill $RPC_PID 2>/dev/null || true
        wait $RPC_PID 2>/dev/null || true
    fi
    rm -f "$STATIC_COMPOSED" "$RPC_COMPOSED"
}
trap cleanup EXIT

RPC_RESULTS=$(wasmtime run -S inherit-network=y "$RPC_COMPOSED" 2>&1)
echo "$RPC_RESULTS"

# =============================================================================
# Summary
# =============================================================================

echo ""
echo "=============================================="
echo "  Summary: Comparison Table"
echo "=============================================="
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""

echo "--- Method 1: Static Compose (wac plug + vfs-adapter) ---"
echo "$STATIC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""

echo "--- Method 2: Host Trait (wasmtime + vfs-host) ---"
echo "$HOST_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""

echo "--- Method 3: RPC (rpc-adapter + vfs-rpc-server) ---"
echo "$RPC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""

echo "=============================================="
echo "  Benchmark Complete"
echo "=============================================="
