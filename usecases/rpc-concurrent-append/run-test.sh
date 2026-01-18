#!/bin/bash
# Concurrent Append Test via VFS RPC
#
# Tests that multiple WASM clients can safely append to the same file
# through the VFS RPC Server without data corruption.
#
# Usage:
#   ./run-test.sh           # Run with defaults (4 clients, 100 appends each)
#   ./run-test.sh 8 500     # Run with 8 clients, 500 appends each

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$SCRIPT_DIR/../.."

# Parameters
NUM_CLIENTS=${1:-3}
APPEND_COUNT=${2:-50}

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "=============================================="
echo "  VFS RPC Concurrent Append Test"
echo "=============================================="
echo ""
echo "Configuration:"
echo "  Clients:         $NUM_CLIENTS"
echo "  Appends/client:  $APPEND_COUNT"
echo "  Expected lines:  $((NUM_CLIENTS * APPEND_COUNT))"
echo ""

# Build components
echo -e "${YELLOW}Building components...${NC}"

# Build RPC server
echo "  Building vfs-rpc-server..."
(cd "$ROOT_DIR" && cargo build -p vfs-rpc-server --target wasm32-wasip2 2>/dev/null)

# Build rpc-adapter
echo "  Building rpc-adapter..."
(cd "$ROOT_DIR" && cargo build -p rpc-adapter --target wasm32-wasip2 2>/dev/null)

# Build append-client
echo "  Building append-client..."
(cd "$SCRIPT_DIR/append-client" && cargo build --release --target wasm32-wasip2 2>/dev/null)

# Build verify-result
echo "  Building verify-result..."
(cd "$SCRIPT_DIR/verify-result" && cargo build --release --target wasm32-wasip2 2>/dev/null)

# Compose WASM modules
echo "  Composing WASM modules..."
COMPOSED_DIR="$SCRIPT_DIR/target"
mkdir -p "$COMPOSED_DIR"

wac plug \
    --plug "$ROOT_DIR/target/wasm32-wasip2/debug/rpc_adapter.wasm" \
    "$SCRIPT_DIR/append-client/target/wasm32-wasip2/release/append-client.wasm" \
    -o "$COMPOSED_DIR/composed-append-client.wasm" 2>/dev/null

wac plug \
    --plug "$ROOT_DIR/target/wasm32-wasip2/debug/rpc_adapter.wasm" \
    "$SCRIPT_DIR/verify-result/target/wasm32-wasip2/release/verify-result.wasm" \
    -o "$COMPOSED_DIR/composed-verify-result.wasm" 2>/dev/null

echo -e "${GREEN}Build complete.${NC}"
echo ""

# Kill any existing server
pkill -f vfs_rpc_server.wasm 2>/dev/null || true
sleep 1

# Start RPC server
echo -e "${YELLOW}Starting VFS RPC Server...${NC}"
wasmtime run -S inherit-network=y -S http \
    "$ROOT_DIR/target/wasm32-wasip2/debug/vfs_rpc_server.wasm" \
    > /tmp/vfs-rpc-concurrent-test.log 2>&1 &
SERVER_PID=$!
sleep 2

# Verify server is running
if ! kill -0 $SERVER_PID 2>/dev/null; then
    echo -e "${RED}ERROR: Server failed to start${NC}"
    cat /tmp/vfs-rpc-concurrent-test.log
    exit 1
fi
echo "Server started (PID: $SERVER_PID)"
echo ""

# Run clients concurrently
echo -e "${YELLOW}Running $NUM_CLIENTS concurrent clients...${NC}"
PIDS=()

for i in $(seq 1 $NUM_CLIENTS); do
    wasmtime run -S inherit-network=y \
        --env CLIENT_ID=$i \
        --env APPEND_COUNT=$APPEND_COUNT \
        "$COMPOSED_DIR/composed-append-client.wasm" 2>&1 | \
        sed "s/^/[Client $i] /" &
    PIDS+=($!)
done

# Wait for all clients to complete
echo "Waiting for all clients to complete..."
FAILED=0
for pid in "${PIDS[@]}"; do
    if ! wait $pid; then
        FAILED=$((FAILED + 1))
    fi
done

if [ $FAILED -gt 0 ]; then
    echo -e "${RED}WARNING: $FAILED client(s) failed${NC}"
fi
echo ""

# Verify results
echo -e "${YELLOW}Verifying results...${NC}"
wasmtime run -S inherit-network=y \
    --env EXPECTED_CLIENTS=$NUM_CLIENTS \
    --env APPEND_COUNT=$APPEND_COUNT \
    "$COMPOSED_DIR/composed-verify-result.wasm"

VERIFY_RESULT=$?

# Show first 20 lines of the file
echo ""
echo -e "${YELLOW}--- First 20 lines of /shared/concurrent.log ---${NC}"
wasmtime run -S inherit-network=y \
    --env SHOW_LINES=20 \
    "$COMPOSED_DIR/composed-verify-result.wasm" 2>/dev/null | head -20 || true

# Cleanup
echo ""
echo "Stopping server..."
kill $SERVER_PID 2>/dev/null || true

if [ $VERIFY_RESULT -eq 0 ]; then
    echo ""
    echo -e "${GREEN}=============================================="
    echo "  TEST PASSED"
    echo "==============================================${NC}"
else
    echo ""
    echo -e "${RED}=============================================="
    echo "  TEST FAILED"
    echo "==============================================${NC}"
    exit 1
fi
