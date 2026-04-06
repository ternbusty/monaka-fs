#!/bin/bash
# tmpfs-vs-vfs Benchmark Transfer Script
# Usage: ./transfer.sh [vm-name]
#
# Transfers built WASM files and run script to a multipass VM.
# Default VM name: composed-petrel

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VM_NAME="${1:-composed-petrel}"
VM_BENCH_DIR="/home/ubuntu/bench-tmpfs-vs-vfs"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

echo -e "${GREEN}=== Transferring tmpfs-vs-vfs Benchmark ===${NC}"
echo "VM: $VM_NAME"
echo "Destination: $VM_BENCH_DIR"
echo ""

# Verify built files exist
for f in "$SCRIPT_DIR/bench-composed.wasm" "$SCRIPT_DIR/bench-raw.wasm"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}[ERROR] $(basename "$f") not found. Run ./build.sh first.${NC}"
        exit 1
    fi
done

# Create directory on VM
multipass exec "$VM_NAME" -- mkdir -p "$VM_BENCH_DIR/wasm"

# Transfer WASM files
echo "Transferring WASM files..."
multipass transfer "$SCRIPT_DIR/bench-composed.wasm" "$VM_NAME:$VM_BENCH_DIR/wasm/bench-composed.wasm"
multipass transfer "$SCRIPT_DIR/bench-raw.wasm" "$VM_NAME:$VM_BENCH_DIR/wasm/bench-raw.wasm"

# Transfer run script
echo "Transferring run script..."
multipass transfer "$SCRIPT_DIR/run.sh" "$VM_NAME:$VM_BENCH_DIR/run.sh"
multipass exec "$VM_NAME" -- chmod +x "$VM_BENCH_DIR/run.sh"

echo ""
echo -e "${GREEN}=== Transfer Complete ===${NC}"
echo "Next: multipass exec $VM_NAME -- $VM_BENCH_DIR/run.sh"
