#!/bin/bash
# Transfer benchmark files to VM using multipass
# Usage: ./transfer-to-vm.sh [VM_NAME]

set -e

VM_NAME="${1:-composed-petrel}"
BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$BENCH_DIR/../.." && pwd)"

echo "=== Building and transferring benchmark files to $VM_NAME ==="

# Check if VM exists and is running
if ! multipass info "$VM_NAME" &>/dev/null; then
    echo "[ERROR] VM '$VM_NAME' not found"
    echo "Available VMs:"
    multipass list
    exit 1
fi

# Create directories on VM
echo ""
echo "=== Creating directories on VM ==="
multipass exec "$VM_NAME" -- mkdir -p /home/ubuntu/halycon-bench/wasm /home/ubuntu/halycon-bench/04

# Build WASM files
echo ""
echo "=== Building WASM files ==="

# Build vfs-rpc-server
echo "Building vfs-rpc-server..."
(cd "$PROJECT_ROOT" && cargo build -p vfs-rpc-server --target wasm32-wasip2 --release)

# Build rpc-adapter
echo "Building rpc-adapter..."
(cd "$PROJECT_ROOT" && cargo build -p rpc-adapter --target wasm32-wasip2 --release)

# Build bench-app
echo "Building bench-app..."
(cd "$BENCH_DIR/bench-app" && cargo build --target wasm32-wasip2 --release)

# Compose bench-app with rpc-adapter
echo "Composing bench-04-rpc.wasm..."
(cd "$PROJECT_ROOT" && wac plug \
    --plug target/wasm32-wasip2/release/rpc_adapter.wasm \
    benchmarks/04-s3fs-vs-s3sync/bench-app/target/wasm32-wasip2/release/bench-s3fs-vs-s3sync.wasm \
    -o benchmarks/04-s3fs-vs-s3sync/bench-04-rpc.wasm)

# Transfer WASM files
echo ""
echo "=== Transferring WASM files ==="
multipass transfer "$PROJECT_ROOT/target/wasm32-wasip2/release/vfs_rpc_server.wasm" "$VM_NAME:/home/ubuntu/halycon-bench/wasm/"
multipass transfer "$BENCH_DIR/bench-04-rpc.wasm" "$VM_NAME:/home/ubuntu/halycon-bench/wasm/"

# Transfer scripts
echo ""
echo "=== Transferring scripts ==="
multipass transfer "$BENCH_DIR/run-bench-vm.sh" "$VM_NAME:/home/ubuntu/halycon-bench/04/"
multipass transfer "$BENCH_DIR/run-bench-vm-realtime.sh" "$VM_NAME:/home/ubuntu/halycon-bench/04/"
multipass transfer "$BENCH_DIR/run-bench-sync-modes.sh" "$VM_NAME:/home/ubuntu/halycon-bench/04/"
multipass transfer "$BENCH_DIR/docker-compose.yml" "$VM_NAME:/home/ubuntu/halycon-bench/04/"

# Transfer .env if exists
if [ -f "$BENCH_DIR/.env" ]; then
    echo "Transferring .env..."
    multipass transfer "$BENCH_DIR/.env" "$VM_NAME:/home/ubuntu/halycon-bench/04/"
fi

# Make scripts executable
multipass exec "$VM_NAME" -- chmod +x /home/ubuntu/halycon-bench/04/*.sh

echo ""
echo "=== Transfer complete ==="
echo ""
echo "WASM files:"
multipass exec "$VM_NAME" -- ls -lh /home/ubuntu/halycon-bench/wasm/
echo ""
echo "Scripts:"
multipass exec "$VM_NAME" -- ls -lh /home/ubuntu/halycon-bench/04/*.sh
echo ""
echo "To run benchmarks:"
echo "  multipass shell $VM_NAME"
echo ""
echo "  # Batch mode (default) vs s3fs:"
echo "  bash /home/ubuntu/halycon-bench/04/run-bench-vm.sh"
echo ""
echo "  # RealTime mode (blocking S3 writes) vs s3fs:"
echo "  bash /home/ubuntu/halycon-bench/04/run-bench-vm-realtime.sh"
echo ""
echo "  # Compare Batch vs RealTime modes:"
echo "  bash /home/ubuntu/halycon-bench/04/run-bench-sync-modes.sh"
