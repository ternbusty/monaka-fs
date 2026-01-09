#!/bin/bash
# CI Cache Demo
#
# Demonstrates multiple CI jobs sharing dependency cache via VFS RPC.
# Each job has dependencies, acquires locks, and reads/writes cache.
#
# Usage:
#   ./usecases/ci-cache-demo/run-demo.sh
#   make run-usecase-ci-cache

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

SERVER_LOG=/tmp/vfs-ci-cache-server.log
SERVER_WASM="target/wasm32-wasip2/debug/vfs_rpc_server.wasm"
JOB_WASM="target/wasm32-wasip2/debug/ci-job-composed.wasm"

# Cleanup function
cleanup() {
    log_info "Cleaning up..."
    pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
}

trap cleanup EXIT

# Check if composed component exists
if [ ! -f "$JOB_WASM" ]; then
    log_error "Composed ci-job not found: $JOB_WASM"
    log_info "Run 'make build-usecase-ci-cache' first"
    exit 1
fi

echo "=============================================="
echo "  CI Cache Demo (RPC-based VFS Sharing)"
echo "=============================================="
echo ""
echo "Scenario:"
echo "  Job1: serde-1.0.0, tokio-1.0.0"
echo "  Job2: serde-1.0.0, anyhow-1.0.0"
echo "  Job3: tokio-1.0.0, anyhow-1.0.0"
echo ""
echo "Jobs share cache via VFS RPC server with per-library locking."
echo ""

# Kill any existing server
cleanup 2>/dev/null || true

# Start VFS RPC server
log_info "Starting VFS RPC Server..."
wasmtime run \
    -S inherit-network=y \
    -S http \
    "$SERVER_WASM" > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!
sleep 1

# Check server started
if ! ps -p $SERVER_PID > /dev/null 2>&1; then
    log_error "Server failed to start!"
    cat "$SERVER_LOG"
    exit 1
fi
log_info "Server started (PID: $SERVER_PID)"
echo ""

log_info "Starting 3 CI jobs in parallel..."
echo ""
echo "----------------------------------------------"

# Run 3 jobs in parallel with different dependencies
# Job1 and Job2 both need serde-1.0.0 (contention)
# Job1 and Job3 both need tokio-1.0.0 (contention)
# Job2 and Job3 both need anyhow-1.0.0 (contention)

wasmtime run -S inherit-network=y --env JOB_ID=1 --env DEPS="serde-1.0.0,tokio-1.0.0" "$JOB_WASM" 2>&1 | grep -v "^\[RPC-ADAPTER\]" &
PID1=$!

wasmtime run -S inherit-network=y --env JOB_ID=2 --env DEPS="serde-1.0.0,anyhow-1.0.0" "$JOB_WASM" 2>&1 | grep -v "^\[RPC-ADAPTER\]" &
PID2=$!

wasmtime run -S inherit-network=y --env JOB_ID=3 --env DEPS="tokio-1.0.0,anyhow-1.0.0" "$JOB_WASM" 2>&1 | grep -v "^\[RPC-ADAPTER\]" &
PID3=$!

# Wait for all jobs to complete
wait $PID1 $PID2 $PID3 2>/dev/null || true

echo "----------------------------------------------"
echo ""
log_info "All jobs completed."

echo ""
log_info "=== Server log (last 20 lines) ==="
tail -20 "$SERVER_LOG" 2>/dev/null || cat "$SERVER_LOG"

echo ""
log_info "=== Demo completed ==="
