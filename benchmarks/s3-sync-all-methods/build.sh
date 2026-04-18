#!/bin/bash
# S3 Sync All Methods Benchmark Build Script
# Usage: ./build.sh
#
# Builds all components on the development machine (macOS).
# After build, use ./transfer.sh to send to a Linux VM.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Building S3 Sync All Methods Benchmark ===${NC}"

# Build bench-app (WASM)
echo -e "${YELLOW}Building bench-app...${NC}"
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2 2>&1 | grep -v "^warning:"

# Cross-compile bench-runtime for Linux ARM64
echo -e "${YELLOW}Cross-compiling bench-runtime for Linux ARM64...${NC}"

if ! command -v cargo-zigbuild &> /dev/null; then
    echo "Installing cargo-zigbuild..."
    cargo install cargo-zigbuild
fi

if ! command -v zig &> /dev/null; then
    echo -e "${RED}[ERROR] zig is not installed. Install with: brew install zig${NC}"
    exit 1
fi

rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true

cd "$SCRIPT_DIR/bench-runtime"
cargo zigbuild --release --target aarch64-unknown-linux-gnu 2>&1 | grep -v "^warning:"

# Build adapters and RPC server with S3 sync
echo -e "${YELLOW}Building adapters and RPC server...${NC}"
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter --features s3-sync 2>&1 | grep -v "^warning:"
cargo build --release --target wasm32-wasip2 -p rpc-adapter 2>&1 | grep -v "^warning:"
cargo build --release --target wasm32-wasip2 -p vfs-rpc-server --features s3-sync 2>&1 | grep -v "^warning:"

# Compose WASM files
echo -e "${YELLOW}Composing WASM components...${NC}"
cd "$SCRIPT_DIR"

BENCH_APP="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-08-app.wasm"
VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"
RPC_ADAPTER="$BUILD_DIR/rpc_adapter.wasm"

wac plug --plug "$VFS_ADAPTER" "$BENCH_APP" -o "$SCRIPT_DIR/bench-wac.wasm"
wac plug --plug "$RPC_ADAPTER" "$BENCH_APP" -o "$SCRIPT_DIR/bench-rpc.wasm"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "  bench-wac.wasm       (app + vfs-adapter with S3 sync)"
echo "  bench-rpc.wasm       (app + rpc-adapter)"
echo "  bench-runtime        (native host trait runner, Linux ARM64)"
echo ""
echo "Next: ./transfer.sh [vm-name]"
