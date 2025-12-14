#!/bin/bash
# Test composed components (build-time composition)
#
# This script tests that composed WASM components work correctly
# without needing the native rpc-fs-runner binary.
#
# Usage:
#   ./examples/rpc/run-composed-test.sh

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

SERVER_LOG=/tmp/vfs-server-composed-test.log

# Kill any existing server processes
cleanup() {
    log_info "Cleaning up..."
    pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
}

trap cleanup EXIT

# Check if composed components exist
if [ ! -f "target/wasm32-wasip2/debug/composed-demo-writer.wasm" ] || \
   [ ! -f "target/wasm32-wasip2/debug/composed-demo-reader.wasm" ]; then
    log_warn "Composed components not found. Building..."
    ./examples/rpc/build-composed.sh
fi

echo "=============================================="
echo "  Composed Component Test (Build-time wac)"
echo "=============================================="
echo ""

# Kill any existing server
cleanup

# Start VFS RPC server
log_info "Starting VFS RPC Server..."
wasmtime run \
    -S inherit-network=y \
    -S http \
    ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!
sleep 2

# Check server started
if ! ps -p $SERVER_PID > /dev/null 2>&1; then
    log_error "Server failed to start!"
    cat "$SERVER_LOG"
    exit 1
fi
log_info "Server started (PID: $SERVER_PID)"

# Run composed demo-writer
echo ""
log_info "Running composed demo-writer..."
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm 2>&1 | grep -v "^\[RPC-ADAPTER\]"

# Run composed demo-reader
echo ""
log_info "Running composed demo-reader..."
wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm 2>&1 | grep -v "^\[RPC-ADAPTER\]"

echo ""
log_info "=== Server log ==="
cat "$SERVER_LOG"

echo ""
log_info "=== Test completed successfully! ==="
echo ""
echo "Build-time composition with wac works!"
echo "No native binary (rpc-fs-runner) was needed."
