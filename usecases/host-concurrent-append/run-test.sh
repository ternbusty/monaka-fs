#!/bin/bash
# Host Trait Concurrent Append Test
#
# Tests that multiple WASM instances running in parallel native threads
# can safely append to the same file through shared VFS (fs-core).
# This validates fs-core's locking implementation (DashMap + per-inode RwLock).
#
# Usage:
#   ./run-test.sh           # Run with defaults (3 clients, 50 appends each)
#   ./run-test.sh 4 100     # Run with 4 clients, 100 appends each

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

echo -e "${YELLOW}Building components...${NC}"

# Build append-client WASM
echo "  Building append-client..."
(cd "$SCRIPT_DIR/append-client" && cargo build --release --target wasm32-wasip2 2>/dev/null)

# Build host-runner
echo "  Building host-runner..."
(cd "$SCRIPT_DIR/host-runner" && cargo build --release 2>/dev/null)

echo -e "${GREEN}Build complete.${NC}"
echo ""

# Run the test
WASM_PATH="$SCRIPT_DIR/append-client/target/wasm32-wasip2/release/append-client.wasm" \
    "$SCRIPT_DIR/host-runner/target/release/host-concurrent-runner" "$NUM_CLIENTS" "$APPEND_COUNT"
