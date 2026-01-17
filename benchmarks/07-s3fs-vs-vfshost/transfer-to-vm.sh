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

echo "=== Building and transferring benchmark 07 files to $VM_NAME ==="

# Check if VM exists and is running
if ! multipass info "$VM_NAME" &>/dev/null; then
    echo "[ERROR] VM '$VM_NAME' not found"
    echo "Available VMs:"
    multipass list
    exit 1
fi

# Build WASM files on host
echo ""
echo "=== Building WASM benchmark app ==="
(cd "$BENCH_DIR/bench-app" && cargo build --release --target wasm32-wasip2)

BENCH_WASM="$BENCH_DIR/bench-app/target/wasm32-wasip2/release/bench-s3fs-vs-vfshost.wasm"

if [ ! -f "$BENCH_WASM" ]; then
    echo "[ERROR] WASM file not found: $BENCH_WASM"
    exit 1
fi

# Cross-compile native runtime for Linux ARM64
echo ""
echo "=== Cross-compiling native runtime for Linux ARM64 ==="

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

# Build
(cd "$BENCH_DIR/bench-runtime" && cargo zigbuild --release --target aarch64-unknown-linux-gnu)

BENCH_RUNTIME="$BENCH_DIR/bench-runtime/target/aarch64-unknown-linux-gnu/release/bench-runtime-s3fs-vs-vfshost"

if [ ! -f "$BENCH_RUNTIME" ]; then
    echo "[ERROR] Runtime binary not found: $BENCH_RUNTIME"
    exit 1
fi

# Create directories on VM
echo ""
echo "=== Creating directories on VM ==="
multipass exec "$VM_NAME" -- mkdir -p \
    /home/ubuntu/halycon-bench/wasm \
    /home/ubuntu/halycon-bench/bin \
    /home/ubuntu/halycon-bench/07

# Transfer files
echo ""
echo "=== Transferring files ==="
multipass transfer "$BENCH_WASM" "$VM_NAME:/home/ubuntu/halycon-bench/wasm/"
multipass transfer "$BENCH_RUNTIME" "$VM_NAME:/home/ubuntu/halycon-bench/bin/"
multipass transfer "$BENCH_DIR/run-bench-vm.sh" "$VM_NAME:/home/ubuntu/halycon-bench/07/"
multipass transfer "$BENCH_DIR/run-bench-vm-realtime.sh" "$VM_NAME:/home/ubuntu/halycon-bench/07/"
multipass transfer "$BENCH_DIR/docker-compose.yml" "$VM_NAME:/home/ubuntu/halycon-bench/07/"

# Transfer .env if exists
if [ -f "$BENCH_DIR/.env" ]; then
    echo "Transferring .env..."
    multipass transfer "$BENCH_DIR/.env" "$VM_NAME:/home/ubuntu/halycon-bench/07/"
fi

# Make files executable
multipass exec "$VM_NAME" -- chmod +x /home/ubuntu/halycon-bench/bin/bench-runtime-s3fs-vs-vfshost
multipass exec "$VM_NAME" -- bash -c 'chmod +x /home/ubuntu/halycon-bench/07/*.sh'

echo ""
echo "=== Transfer complete ==="
echo ""
echo "WASM files:"
multipass exec "$VM_NAME" -- ls -lh /home/ubuntu/halycon-bench/wasm/
echo ""
echo "Native binaries:"
multipass exec "$VM_NAME" -- ls -lh /home/ubuntu/halycon-bench/bin/
echo ""
echo "Scripts:"
multipass exec "$VM_NAME" -- bash -c 'ls -lh /home/ubuntu/halycon-bench/07/*.sh'
echo ""
echo "To run benchmarks:"
echo "  multipass shell $VM_NAME"
echo ""
echo "  # Batch mode (default):"
echo "  bash /home/ubuntu/halycon-bench/07/run-bench-vm.sh"
echo ""
echo "  # RealTime mode:"
echo "  bash /home/ubuntu/halycon-bench/07/run-bench-vm-realtime.sh"
echo ""
