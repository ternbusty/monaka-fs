#!/bin/bash
# Benchmark 04: In-memory VFS vs S3 Sync
# Compares pure in-memory VFS (no persistence) with S3-synced VFS
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"

echo "=========================================="
echo "  Benchmark 04: In-memory vs S3 Sync"
echo "=========================================="
echo ""

echo "=== Building benchmark app ==="
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2

BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-s3fs-vs-s3sync.wasm"

echo ""
echo "=== Building adapters and server ==="
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p rpc-adapter -p vfs-rpc-server

RPC_ADAPTER="$BUILD_DIR/rpc_adapter.wasm"
RPC_SERVER="$BUILD_DIR/vfs_rpc_server.wasm"

echo ""
echo "=== Composing WASM component ==="
RPC_COMPOSED="$SCRIPT_DIR/bench-rpc.wasm"

wac plug --plug "$RPC_ADAPTER" "$BENCH_WASM" -o "$RPC_COMPOSED"

# ==========================================
#   RPC VFS without S3
# ==========================================
echo ""
echo "=========================================="
echo "  Running: RPC VFS (no S3)"
echo "=========================================="
echo ""

echo "Starting vfs-rpc-server without S3..."
wasmtime run -S inherit-network=y -S http "$RPC_SERVER" &
RPC_PID_NO_S3=$!
sleep 2

if ! kill -0 $RPC_PID_NO_S3 2>/dev/null; then
    echo "[ERROR] Failed to start RPC server"
    exit 1
fi

NO_S3_RESULTS=$(wasmtime run -S inherit-network=y "$RPC_COMPOSED" 2>&1)
echo "$NO_S3_RESULTS"

kill $RPC_PID_NO_S3 2>/dev/null || true
wait $RPC_PID_NO_S3 2>/dev/null || true

# ==========================================
#   RPC VFS with S3 Sync
# ==========================================
echo ""
echo "=========================================="
echo "  Running: RPC VFS with S3 Sync"
echo "=========================================="
echo ""

echo "=== Checking LocalStack ==="
chmod +x "$SCRIPT_DIR/localstack-init/create-bucket.sh"
cd "$SCRIPT_DIR"

# Check if LocalStack is already running
if curl -s http://localhost:4566/_localstack/health | grep -q '"s3"'; then
    echo "LocalStack is already running"
    LOCALSTACK_STARTED_BY_US=false
else
    echo "Starting LocalStack..."
    docker compose up -d localstack
    LOCALSTACK_STARTED_BY_US=true

    echo "Waiting for LocalStack to be ready..."
    for i in {1..30}; do
        if curl -s http://localhost:4566/_localstack/health | grep -q '"s3"'; then
            echo "LocalStack is ready"
            break
        fi
        if [ $i -eq 30 ]; then
            echo "[ERROR] LocalStack S3 is not ready"
            docker compose down
            exit 1
        fi
        sleep 1
    done
fi

# Ensure bucket exists (using curl instead of aws cli)
if ! curl -s -o /dev/null -w "%{http_code}" http://localhost:4566/halycon-bench | grep -q "200"; then
    echo "Creating S3 bucket: halycon-bench"
    curl -s -X PUT http://localhost:4566/halycon-bench > /dev/null
fi

# Cleanup function
cleanup() {
    if [ -n "$RPC_PID" ] && kill -0 $RPC_PID 2>/dev/null; then
        echo "Stopping RPC server..."
        kill $RPC_PID 2>/dev/null || true
        wait $RPC_PID 2>/dev/null || true
    fi
    cd "$SCRIPT_DIR"
    if [ "$LOCALSTACK_STARTED_BY_US" = true ]; then
        docker compose down 2>/dev/null || true
    fi
    rm -f "$RPC_COMPOSED"
}
trap cleanup EXIT

# Start RPC server with S3 sync enabled
echo "Starting vfs-rpc-server with S3 sync..."
wasmtime run -S inherit-network=y -S http \
    --env VFS_S3_BUCKET=halycon-bench \
    --env VFS_S3_PREFIX=vfs/ \
    --env AWS_ENDPOINT_URL=http://localhost:4566 \
    --env AWS_ACCESS_KEY_ID=test \
    --env AWS_SECRET_ACCESS_KEY=test \
    --env AWS_REGION=ap-northeast-1 \
    "$RPC_SERVER" &
RPC_PID=$!

sleep 2

if ! kill -0 $RPC_PID 2>/dev/null; then
    echo "[ERROR] Failed to start RPC server"
    docker compose down
    exit 1
fi

# Measure total time including S3 sync
SYNC_START=$(date +%s%N)

RPC_RESULTS=$(wasmtime run -S inherit-network=y "$RPC_COMPOSED" 2>&1)
echo "$RPC_RESULTS"

# Stop server to trigger S3 sync
echo ""
echo "Triggering S3 sync (stopping server)..."
kill $RPC_PID 2>/dev/null || true
wait $RPC_PID 2>/dev/null || true
RPC_PID=""

SYNC_END=$(date +%s%N)
SYNC_DURATION=$(echo "scale=3; ($SYNC_END - $SYNC_START) / 1000000" | bc)
echo "[SYNC] Total time including S3 sync: ${SYNC_DURATION}ms"

# ==========================================
#   Results Summary
# ==========================================
echo ""
echo "=========================================="
echo "  Results Summary"
echo "=========================================="
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""

echo "--- RPC VFS (no S3) ---"
echo "$NO_S3_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""

echo "--- RPC VFS with S3 Sync ---"
echo "$RPC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "Note: Both use RPC. Difference shows S3 sync overhead."
echo "S3 sync occurs at session end (deferred persistence)."
echo "Total time including S3 sync: ${SYNC_DURATION}ms"

echo ""
echo "=== Benchmark 04 Complete ==="
