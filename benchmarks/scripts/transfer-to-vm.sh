#!/bin/bash
# Transfer benchmark files to VM
# Usage: ./scripts/transfer-to-vm.sh

set -e

VM_NAME="composed-petrel"
VM_BENCH_DIR="/home/ubuntu/halycon-bench"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BENCHMARKS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Transferring Benchmark Files to VM ==="
echo "VM: $VM_NAME"
echo "Destination: $VM_BENCH_DIR"
echo ""

# Ensure directories exist on VM
echo "Creating directories on VM..."
multipass exec "$VM_NAME" -- mkdir -p "$VM_BENCH_DIR"/{wasm,scripts,01,03,04/results,04/localstack-init}

# Transfer WASM files
echo ""
echo "=== Transferring WASM files ==="

transfer_wasm() {
    local src="$1"
    local dest="$2"
    if [ -f "$src" ]; then
        echo "  $(basename "$src")"
        multipass transfer "$src" "$VM_NAME:$dest"
    else
        echo "  [SKIP] $(basename "$src") not found"
    fi
}

# Core components
transfer_wasm "$SCRIPT_DIR/vfs_rpc_server.wasm" "$VM_BENCH_DIR/wasm/vfs_rpc_server.wasm"

# Benchmark: tmpfs-vs-vfs
transfer_wasm "$BENCHMARKS_DIR/tmpfs-vs-vfs/bench-composed.wasm" "$VM_BENCH_DIR/wasm/bench-composed.wasm"

# Benchmark 03
transfer_wasm "$BENCHMARKS_DIR/03-local-vs-rpc/bench-03-local.wasm" "$VM_BENCH_DIR/wasm/bench-03-local.wasm"
transfer_wasm "$BENCHMARKS_DIR/03-local-vs-rpc/bench-03-rpc.wasm" "$VM_BENCH_DIR/wasm/bench-03-rpc.wasm"

# Benchmark 04
transfer_wasm "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/bench-04-rpc.wasm" "$VM_BENCH_DIR/wasm/bench-04-rpc.wasm"

# Also transfer the raw (non-composed) WASM for tmpfs baseline
transfer_wasm "$BENCHMARKS_DIR/tmpfs-vs-vfs/bench-raw.wasm" "$VM_BENCH_DIR/wasm/bench-raw.wasm"

# Transfer VM benchmark scripts
echo ""
echo "=== Transferring VM scripts ==="

# Transfer with numbered names for run-all-on-vm.sh
transfer_script() {
    local bench_dir="$1"
    local dest_name="$2"
    local src="$BENCHMARKS_DIR/$bench_dir/run-bench-vm.sh"
    if [ -f "$src" ]; then
        echo "  $bench_dir/run-bench-vm.sh -> $dest_name"
        multipass transfer "$src" "$VM_NAME:$VM_BENCH_DIR/scripts/$dest_name"
    else
        echo "  [SKIP] $bench_dir/run-bench-vm.sh not found"
    fi
}

transfer_script "tmpfs-vs-vfs" "run-bench-tmpfs-vs-vfs.sh"
transfer_script "03-local-vs-rpc" "run-bench-vm-03.sh"
transfer_script "04-s3fs-vs-s3sync" "run-bench-vm-04.sh"

# Transfer benchmark 04 specific files
echo ""
echo "=== Transferring benchmark 04 configs ==="

if [ -f "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/docker-compose.yml" ]; then
    echo "  docker-compose.yml"
    multipass transfer "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/docker-compose.yml" "$VM_NAME:$VM_BENCH_DIR/04/docker-compose.yml"
fi

if [ -f "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/localstack-init/create-bucket.sh" ]; then
    echo "  localstack-init/create-bucket.sh"
    multipass transfer "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/localstack-init/create-bucket.sh" "$VM_NAME:$VM_BENCH_DIR/04/localstack-init/create-bucket.sh"
fi

# Make scripts executable on VM
echo ""
echo "=== Setting permissions ==="
multipass exec "$VM_NAME" -- chmod +x "$VM_BENCH_DIR/scripts/"*.sh 2>/dev/null || true
multipass exec "$VM_NAME" -- chmod +x "$VM_BENCH_DIR/04/localstack-init/"*.sh 2>/dev/null || true

# Verify transfer
echo ""
echo "=== Verification ==="
multipass exec "$VM_NAME" -- ls -la "$VM_BENCH_DIR/wasm/"
multipass exec "$VM_NAME" -- ls -la "$VM_BENCH_DIR/scripts/"

echo ""
echo "=== Transfer Complete ==="
echo ""
echo "Next: ./scripts/run-all-on-vm.sh"
