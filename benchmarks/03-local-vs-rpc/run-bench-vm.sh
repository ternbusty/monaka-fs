#!/bin/bash
# Benchmark 03: Local VFS vs RPC (VM version)
# This script runs on the Linux VM

BENCH_DIR="/home/ubuntu/halycon-bench"
WASM_DIR="$BENCH_DIR/wasm"
SERVER_LOG="/tmp/vfs-rpc-server.log"

export PATH="$HOME/.wasmtime/bin:$PATH"

echo "=========================================="
echo "  Benchmark 03: Local VFS vs RPC"
echo "=========================================="
echo ""

# Verify wasmtime is available
if ! command -v wasmtime &> /dev/null; then
    echo "[ERROR] wasmtime not found. Run vm-setup.sh first."
    exit 1
fi

# Verify WASM files exist
for wasm in bench-03-local.wasm bench-03-rpc.wasm vfs_rpc_server.wasm; do
    if [ ! -f "$WASM_DIR/$wasm" ]; then
        echo "[ERROR] $WASM_DIR/$wasm not found"
        exit 1
    fi
done

# Cleanup function
cleanup() {
    echo "Stopping RPC server..."
    # Kill any wasmtime process running vfs_rpc_server
    pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
    # Also try with saved PID
    if [ -n "$SERVER_PID" ]; then
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

echo "=== Running: Local VFS (wac plug) ==="
echo ""

LOCAL_RESULTS=$(wasmtime run "$WASM_DIR/bench-03-local.wasm" 2>&1)
echo "$LOCAL_RESULTS"

echo ""
echo "=== Running: RPC VFS ==="
echo ""

# Start vfs-rpc-server
echo "Starting vfs-rpc-server..."
wasmtime run -S inherit-network=y -S http "$WASM_DIR/vfs_rpc_server.wasm" > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!

# Wait for server to start listening on port 9000
echo "Waiting for server to start..."
for i in {1..10}; do
    if ss -tlnp 2>/dev/null | grep -q ":9000 "; then
        echo "RPC server started (PID: $SERVER_PID)"
        break
    fi
    if [ $i -eq 10 ]; then
        echo "[ERROR] Failed to start RPC server (port 9000 not listening)"
        echo "Server log:"
        cat "$SERVER_LOG"
        exit 1
    fi
    sleep 1
done
echo ""

# Run RPC benchmark and save results to file (direct output, not captured in variable)
RPC_OUTPUT="/tmp/rpc-bench-results.txt"
wasmtime run -S inherit-network=y "$WASM_DIR/bench-03-rpc.wasm" 2>&1 | tee "$RPC_OUTPUT"

echo ""
echo "=========================================="
echo "  Results Summary"
echo "=========================================="
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""
echo "--- Local VFS (wac plug) ---"
echo "$LOCAL_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- RPC VFS ---"
grep "^\[RESULT\]" "$RPC_OUTPUT" 2>/dev/null | sed 's/\[RESULT\] //' || echo "No RPC results available"

echo ""
echo "=== Benchmark 03 Complete ==="
