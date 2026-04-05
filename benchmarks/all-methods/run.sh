#!/bin/bash
# All Methods Benchmark Runner
# Usage: ./run.sh
#
# Compares three VFS implementation methods:
#   1. Static Compose (wac plug + vfs-adapter)
#   2. Host Trait (wasmtime + vfs-host)
#   3. RPC (rpc-adapter + vfs-rpc-server)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"
BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-all-methods.wasm"
RPC_SERVER="$BUILD_DIR/vfs_rpc_server.wasm"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Verify built files
for f in "$SCRIPT_DIR/bench-static.wasm" "$SCRIPT_DIR/bench-rpc.wasm" "$RPC_SERVER"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}[ERROR] $(basename "$f") not found. Run ./build.sh first.${NC}"
        exit 1
    fi
done

BENCH_RUNNER="$SCRIPT_DIR/bench-runner/target/release/bench-runner"
if [ ! -f "$BENCH_RUNNER" ]; then
    echo -e "${RED}[ERROR] bench-runner not found. Run ./build.sh first.${NC}"
    exit 1
fi

echo -e "${GREEN}==============================================${NC}"
echo -e "${GREEN}  Benchmark: All VFS Methods Comparison${NC}"
echo -e "${GREEN}==============================================${NC}"
echo ""

# --- Method 1: Static Compose ---
echo -e "${YELLOW}=== Method 1: Static Compose (wac plug) ===${NC}"
echo ""
STATIC_RESULTS=$(wasmtime run "$SCRIPT_DIR/bench-static.wasm" 2>&1)
echo "$STATIC_RESULTS"

# --- Method 2: Host Trait ---
echo ""
echo -e "${YELLOW}=== Method 2: Host Trait (vfs-host) ===${NC}"
echo ""
HOST_RESULTS=$("$BENCH_RUNNER" "$BENCH_WASM" 2>&1)
echo "$HOST_RESULTS"

# --- Method 3: RPC ---
echo ""
echo -e "${YELLOW}=== Method 3: RPC (vfs-rpc-server) ===${NC}"
echo ""

# Start RPC server in background
echo "Starting vfs-rpc-server..."
wasmtime run -S inherit-network=y -S http "$RPC_SERVER" > /tmp/vfs-server-bench.log 2>&1 &
RPC_PID=$!

cleanup() {
    if [ -n "$RPC_PID" ] && kill -0 $RPC_PID 2>/dev/null; then
        kill $RPC_PID 2>/dev/null || true
        wait $RPC_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

sleep 2

if ! kill -0 $RPC_PID 2>/dev/null; then
    echo -e "${RED}[ERROR] Failed to start RPC server${NC}"
    cat /tmp/vfs-server-bench.log
    exit 1
fi

RPC_RESULTS=$(wasmtime run -S inherit-network=y "$SCRIPT_DIR/bench-rpc.wasm" 2>&1)
echo "$RPC_RESULTS"

# --- Summary ---
echo ""
echo -e "${GREEN}==============================================${NC}"
echo -e "${GREEN}  Results Summary${NC}"
echo -e "${GREEN}==============================================${NC}"
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""
echo "--- Static Compose ---"
echo "$STATIC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- Host Trait ---"
echo "$HOST_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- RPC ---"
echo "$RPC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'

echo ""
echo -e "${GREEN}=== Benchmark Complete ===${NC}"
