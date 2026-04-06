#!/bin/bash
# tmpfs-vs-vfs Benchmark Build Script
# Usage: ./build.sh
#
# Builds the WASM benchmark app and composes it with the VFS adapter.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Building tmpfs-vs-vfs Benchmark ===${NC}"

# Build vfs-adapter
echo -e "${YELLOW}Building vfs-adapter...${NC}"
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter 2>&1 | grep -v "^warning:"

VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"
if [ ! -f "$VFS_ADAPTER" ]; then
    echo -e "${RED}[ERROR] vfs-adapter build failed${NC}"
    exit 1
fi

# Build bench-app (standalone workspace)
echo -e "${YELLOW}Building bench-app...${NC}"
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2 2>&1 | grep -v "^warning:"

BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-tmpfs-vs-vfs.wasm"
if [ ! -f "$BENCH_WASM" ]; then
    echo -e "${RED}[ERROR] bench-app build failed${NC}"
    exit 1
fi

# Compose with vfs-adapter
echo -e "${YELLOW}Composing with vfs-adapter...${NC}"
wac plug --plug "$VFS_ADAPTER" "$BENCH_WASM" -o "$SCRIPT_DIR/bench-composed.wasm"

# Copy raw WASM for host baseline tests
cp "$BENCH_WASM" "$SCRIPT_DIR/bench-raw.wasm"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "  bench-composed.wasm  (app + vfs-adapter, for VFS benchmark)"
echo "  bench-raw.wasm       (raw app, for ext4/tmpfs host baseline)"
