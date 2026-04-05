#!/bin/bash
# Build all benchmark WASM files for VM execution
# Usage: ./scripts/build-for-vm.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BENCHMARKS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

echo "=== Building Halycon Benchmarks for VM ==="
echo "Root: $ROOT_DIR"
echo "Build: $BUILD_DIR"
echo ""

# Build core components
echo "=== Building core components ==="
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter -p rpc-adapter -p vfs-rpc-server

VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"
RPC_ADAPTER="$BUILD_DIR/rpc_adapter.wasm"
RPC_SERVER="$BUILD_DIR/vfs_rpc_server.wasm"

# Verify core builds
for f in "$VFS_ADAPTER" "$RPC_ADAPTER" "$RPC_SERVER"; do
    if [ ! -f "$f" ]; then
        echo "[ERROR] Missing: $f"
        exit 1
    fi
done
echo "Core components built successfully"

# Build benchmark: tmpfs-vs-vfs (standalone workspace - builds to its own target/)
echo ""
echo "=== Building benchmark: tmpfs-vs-vfs ==="
cd "$BENCHMARKS_DIR/tmpfs-vs-vfs/bench-app"
cargo build --release --target wasm32-wasip2
BENCH_TMPFS_WASM="$BENCHMARKS_DIR/tmpfs-vs-vfs/bench-app/target/wasm32-wasip2/release/bench-tmpfs-vs-vfs.wasm"

# Build benchmark 03 (standalone workspace)
echo ""
echo "=== Building benchmark 03: local-vs-rpc ==="
cd "$BENCHMARKS_DIR/03-local-vs-rpc/bench-app"
cargo build --release --target wasm32-wasip2
BENCH03_WASM="$BENCHMARKS_DIR/03-local-vs-rpc/bench-app/target/wasm32-wasip2/release/bench-local-vs-rpc.wasm"

# Build benchmark 04 (standalone workspace)
echo ""
echo "=== Building benchmark 04: s3fs-vs-s3sync ==="
cd "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/bench-app"
cargo build --release --target wasm32-wasip2
BENCH04_WASM="$BENCHMARKS_DIR/04-s3fs-vs-s3sync/bench-app/target/wasm32-wasip2/release/bench-s3fs-vs-s3sync.wasm"

# Compose WASM components
echo ""
echo "=== Composing WASM components ==="

# Benchmark: tmpfs-vs-vfs (app + vfs-adapter)
echo "Composing benchmark: tmpfs-vs-vfs..."
wac plug --plug "$VFS_ADAPTER" "$BENCH_TMPFS_WASM" -o "$BENCHMARKS_DIR/tmpfs-vs-vfs/bench-composed.wasm"

# Benchmark 03: app + vfs-adapter (local) and app + rpc-adapter (rpc)
echo "Composing benchmark 03 (local)..."
wac plug --plug "$VFS_ADAPTER" "$BENCH03_WASM" -o "$BENCHMARKS_DIR/03-local-vs-rpc/bench-03-local.wasm"

echo "Composing benchmark 03 (rpc)..."
wac plug --plug "$RPC_ADAPTER" "$BENCH03_WASM" -o "$BENCHMARKS_DIR/03-local-vs-rpc/bench-03-rpc.wasm"

# Benchmark 04: app + rpc-adapter
echo "Composing benchmark 04..."
wac plug --plug "$RPC_ADAPTER" "$BENCH04_WASM" -o "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/bench-04-rpc.wasm"

# Copy vfs-rpc-server to benchmarks dir for transfer
cp "$RPC_SERVER" "$BENCHMARKS_DIR/scripts/vfs_rpc_server.wasm"

echo ""
echo "=== Build Complete ==="
echo ""
echo "Built files:"
ls -la "$BENCHMARKS_DIR/tmpfs-vs-vfs/"*.wasm 2>/dev/null || true
ls -la "$BENCHMARKS_DIR/03-local-vs-rpc/"*.wasm 2>/dev/null || true
ls -la "$BENCHMARKS_DIR/04-s3fs-vs-s3sync/"*.wasm 2>/dev/null || true
ls -la "$BENCHMARKS_DIR/scripts/"*.wasm 2>/dev/null || true

echo ""
echo "Next: ./scripts/transfer-to-vm.sh"
