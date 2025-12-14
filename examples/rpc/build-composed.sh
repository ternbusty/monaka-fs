#!/bin/bash
# Build composed components using wac plug
#
# This script builds demo-writer and demo-reader composed with rpc-adapter,
# eliminating the need for the native rpc-fs-runner binary.
#
# Usage:
#   ./examples/rpc/build-composed.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_DIR"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

# Check for wac-cli
if ! command -v wac &> /dev/null; then
    log_warn "wac-cli not found. Installing..."
    cargo install wac-cli
fi

# Build individual components
log_info "Building WASM components..."
cargo build --target wasm32-wasip2 \
    -p rpc-adapter \
    -p demo-writer \
    -p demo-reader \
    -p vfs-rpc-server

# Compose applications with rpc-adapter
log_info "Composing demo-writer with rpc-adapter..."
wac plug \
    --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
    target/wasm32-wasip2/debug/demo-writer.wasm \
    -o target/wasm32-wasip2/debug/composed-demo-writer.wasm

log_info "Composing demo-reader with rpc-adapter..."
wac plug \
    --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
    target/wasm32-wasip2/debug/demo-reader.wasm \
    -o target/wasm32-wasip2/debug/composed-demo-reader.wasm

log_info "Build complete!"
echo ""
echo "Composed components created:"
echo "  - target/wasm32-wasip2/debug/composed-demo-writer.wasm"
echo "  - target/wasm32-wasip2/debug/composed-demo-reader.wasm"
echo ""
echo "To run:"
echo "  # Terminal 1: Start VFS RPC Server"
echo "  wasmtime run -S inherit-network=y -S http ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm"
echo ""
echo "  # Terminal 2: Run composed apps (no native binary needed!)"
echo "  wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm"
echo "  wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm"
