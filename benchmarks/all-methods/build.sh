#!/bin/bash
# All Methods Benchmark Build Script
# Usage: ./build.sh
#
# Builds all components needed for the three VFS method comparison.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Building All Methods Benchmark ===${NC}"

# Build benchmark WASM app
echo -e "${YELLOW}Building bench-app...${NC}"
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2 2>&1 | grep -v "^warning:"
BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-all-methods.wasm"

# Build adapters and RPC server
echo -e "${YELLOW}Building adapters and RPC server...${NC}"
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter -p rpc-adapter -p vfs-rpc-server 2>&1 | grep -v "^warning:"

VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"
RPC_ADAPTER="$BUILD_DIR/rpc_adapter.wasm"

# Build bench-runner (native, for host trait method)
echo -e "${YELLOW}Building bench-runner (host trait)...${NC}"
cd "$SCRIPT_DIR/bench-runner"
cargo build --release 2>&1 | grep -v "^warning:"

# Compose WASM components
echo -e "${YELLOW}Composing WASM components...${NC}"
wac plug --plug "$VFS_ADAPTER" "$BENCH_WASM" -o "$SCRIPT_DIR/bench-static.wasm"
wac plug --plug "$RPC_ADAPTER" "$BENCH_WASM" -o "$SCRIPT_DIR/bench-rpc.wasm"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "  bench-static.wasm   (static compose: app + vfs-adapter)"
echo "  bench-rpc.wasm      (RPC: app + rpc-adapter)"
echo "  bench-runner        (native host trait runner)"
