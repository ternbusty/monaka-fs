#!/bin/bash
# S3 Sync All Methods Benchmark Transfer Script
# Usage: ./transfer.sh [vm-name]
#
# Transfers built binaries, WASM files, and scripts to a multipass VM.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
VM_NAME="${1:-composed-petrel}"
VM_DIR="/home/ubuntu/bench-s3-sync-all-methods"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${GREEN}=== Transferring S3 Sync All Methods Benchmark ===${NC}"
echo "VM: $VM_NAME"
echo "Destination: $VM_DIR"
echo ""

# Verify built files
BENCH_APP="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-08-app.wasm"
BENCH_RUNTIME="$SCRIPT_DIR/bench-runtime/target/aarch64-unknown-linux-gnu/release/bench-runtime-08"
RPC_SERVER="$ROOT_DIR/target/wasm32-wasip2/release/vfs_rpc_server.wasm"

for f in "$BENCH_APP" "$BENCH_RUNTIME" "$SCRIPT_DIR/bench-wac.wasm" "$SCRIPT_DIR/bench-rpc.wasm" "$RPC_SERVER"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}[ERROR] $(basename "$f") not found. Run ./build.sh first.${NC}"
        exit 1
    fi
done

# Create directories on VM
multipass exec "$VM_NAME" -- mkdir -p "$VM_DIR"/{wasm,bin}

# Transfer WASM files
echo "Transferring WASM files..."
multipass transfer "$BENCH_APP" "$VM_NAME:$VM_DIR/wasm/bench-app.wasm"
multipass transfer "$SCRIPT_DIR/bench-wac.wasm" "$VM_NAME:$VM_DIR/wasm/bench-wac.wasm"
multipass transfer "$SCRIPT_DIR/bench-rpc.wasm" "$VM_NAME:$VM_DIR/wasm/bench-rpc.wasm"
multipass transfer "$RPC_SERVER" "$VM_NAME:$VM_DIR/wasm/vfs-rpc-server.wasm"

# Transfer runtime binary
echo "Transferring runtime binary..."
multipass transfer "$BENCH_RUNTIME" "$VM_NAME:$VM_DIR/bin/bench-runtime"

# Transfer scripts
echo "Transferring scripts..."
multipass transfer "$SCRIPT_DIR/run.sh" "$VM_NAME:$VM_DIR/"

# Transfer .env if exists
if [ -f "$SCRIPT_DIR/.env" ]; then
    echo "Transferring .env..."
    multipass transfer "$SCRIPT_DIR/.env" "$VM_NAME:$VM_DIR/"
fi

# Set permissions
multipass exec "$VM_NAME" -- chmod +x "$VM_DIR/run.sh" "$VM_DIR/bin/bench-runtime"

echo ""
echo -e "${GREEN}=== Transfer Complete ===${NC}"
echo "Next: multipass exec $VM_NAME -- $VM_DIR/run.sh"
