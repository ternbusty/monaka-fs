#!/bin/bash
# Transfer benchmark files to VM using multipass
# Usage: ./transfer-to-vm.sh [VM_NAME]
#
# This script:
# 1. Builds WASM benchmark app on host
# 2. Cross-compiles native runtime for Linux ARM64
# 3. Transfers binaries and scripts to VM

set -e

VM_NAME="${1:-composed-petrel}"
BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$BENCH_DIR/../.." && pwd)"

echo "=== Building and transferring benchmark 08 files to $VM_NAME ==="

# Check if VM exists and is running
if ! multipass info "$VM_NAME" &>/dev/null; then
    echo "[ERROR] VM '$VM_NAME' not found"
    echo "Available VMs:"
    multipass list
    exit 1
fi

REMOTE_DIR="/home/ubuntu/halycon-bench"

# Create remote directories
echo ""
echo "=== Creating remote directories ==="
multipass exec "$VM_NAME" -- mkdir -p "$REMOTE_DIR"/{08,wasm,bin}
multipass exec "$VM_NAME" -- mkdir -p "$REMOTE_DIR"/08/localstack-init

# Build bench-app
echo ""
echo "=== Building bench-app ==="
cd "$BENCH_DIR/bench-app"
cargo build --release --target wasm32-wasip2

# Cross-compile bench-runtime for Linux ARM64
echo ""
echo "=== Cross-compiling bench-runtime for Linux ARM64 ==="

# Check if cargo-zigbuild is installed
if ! command -v cargo-zigbuild &> /dev/null; then
    echo "Installing cargo-zigbuild..."
    cargo install cargo-zigbuild
fi

# Check if zig is installed
if ! command -v zig &> /dev/null; then
    echo "[ERROR] zig is not installed. Install with: brew install zig"
    exit 1
fi

# Add target if not present
rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true

cd "$BENCH_DIR/bench-runtime"
cargo zigbuild --release --target aarch64-unknown-linux-gnu

# Build vfs-adapter with s3-sync
echo ""
echo "=== Building vfs-adapter ==="
cd "$PROJECT_ROOT"
cargo build --release --target wasm32-wasip2 -p vfs-adapter --features s3-sync

# Build rpc-adapter
echo ""
echo "=== Building rpc-adapter ==="
cargo build --release --target wasm32-wasip2 -p rpc-adapter

# Build vfs-rpc-server
echo ""
echo "=== Building vfs-rpc-server ==="
cargo build --release --target wasm32-wasip2 -p vfs-rpc-server

# Compose WASM files
echo ""
echo "=== Composing WASM files ==="
cd "$BENCH_DIR"

BENCH_APP="$BENCH_DIR/bench-app/target/wasm32-wasip2/release/bench-08-app.wasm"
VFS_ADAPTER="$PROJECT_ROOT/target/wasm32-wasip2/release/vfs_adapter.wasm"
RPC_ADAPTER="$PROJECT_ROOT/target/wasm32-wasip2/release/rpc_adapter.wasm"
VFS_RPC_SERVER="$PROJECT_ROOT/target/wasm32-wasip2/release/vfs_rpc_server.wasm"

echo "Creating bench-08-wac.wasm..."
wac plug --plug "$VFS_ADAPTER" "$BENCH_APP" -o bench-08-wac.wasm

echo "Creating bench-08-rpc.wasm..."
wac plug --plug "$RPC_ADAPTER" "$BENCH_APP" -o bench-08-rpc.wasm

# Transfer files
echo ""
echo "=== Transferring files ==="

echo "Transferring WASM files..."
multipass transfer "$BENCH_APP" "$VM_NAME:$REMOTE_DIR/wasm/bench-08-app.wasm"
multipass transfer bench-08-wac.wasm "$VM_NAME:$REMOTE_DIR/wasm/bench-08-wac.wasm"
multipass transfer bench-08-rpc.wasm "$VM_NAME:$REMOTE_DIR/wasm/bench-08-rpc.wasm"
multipass transfer "$VFS_RPC_SERVER" "$VM_NAME:$REMOTE_DIR/wasm/vfs-rpc-server.wasm"

echo "Transferring runtime binary..."
multipass transfer "$BENCH_DIR/bench-runtime/target/aarch64-unknown-linux-gnu/release/bench-runtime-08" "$VM_NAME:$REMOTE_DIR/bin/bench-runtime-08"

echo "Transferring scripts..."
multipass transfer run-bench-vm.sh "$VM_NAME:$REMOTE_DIR/08/"
multipass transfer docker-compose.yml "$VM_NAME:$REMOTE_DIR/08/"
multipass transfer localstack-init/init-s3.sh "$VM_NAME:$REMOTE_DIR/08/localstack-init/"

# Transfer .env if exists
if [ -f .env ]; then
    echo "Transferring .env..."
    multipass transfer .env "$VM_NAME:$REMOTE_DIR/08/"
fi

# Set permissions
echo ""
echo "=== Setting permissions ==="
multipass exec "$VM_NAME" -- chmod +x "$REMOTE_DIR/08/run-bench-vm.sh"
multipass exec "$VM_NAME" -- chmod +x "$REMOTE_DIR/bin/bench-runtime-08"
multipass exec "$VM_NAME" -- chmod +x "$REMOTE_DIR/08/localstack-init/init-s3.sh"

# Cleanup local composed files
rm -f bench-08-wac.wasm bench-08-rpc.wasm

echo ""
echo "=========================================="
echo "  Transfer Complete"
echo "=========================================="
echo ""
echo "To run the benchmark on the VM:"
echo "  multipass shell $VM_NAME"
echo "  cd $REMOTE_DIR/08"
echo "  ./run-bench-vm.sh"
echo ""
echo "Or directly:"
echo "  multipass exec $VM_NAME -- bash -c 'cd $REMOTE_DIR/08 && ./run-bench-vm.sh'"
echo ""
