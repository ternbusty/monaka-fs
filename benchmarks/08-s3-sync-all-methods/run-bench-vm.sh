#!/bin/bash
# Benchmark 08: S3 Sync - All Methods Comparison (VM version)
#
# Compares four methods with full S3 passthrough:
# 1. s3fs-fuse: Direct S3 mount via FUSE
# 2. vfs-host: Host trait implementation with S3 sync
# 3. wac-plug: WASM composition with vfs-adapter
# 4. rpc: RPC-based VFS with vfs-rpc-server
#
# All VFS implementations run in S3 passthrough mode:
# - VFS_SYNC_MODE=realtime (immediate S3 sync on write)
# - VFS_READ_MODE=s3 (read-through from S3)
# - VFS_METADATA_MODE=s3 (HEAD request on open)
#
# This script runs on a Linux VM with s3fs installed

set -e

BENCH_DIR="/home/ubuntu/halycon-bench"
WASM_DIR="$BENCH_DIR/wasm"
BIN_DIR="$BENCH_DIR/bin"
COMPOSE_DIR="$BENCH_DIR/08"
S3FS_MOUNT="/tmp/s3fs-bench"
ENV_FILE="$COMPOSE_DIR/.env"

# Load .env file if exists (for real S3/GCS)
USE_LOCALSTACK=true
if [ -f "$ENV_FILE" ]; then
    echo "Loading credentials from $ENV_FILE"
    set -a
    source "$ENV_FILE"
    set +a
    USE_LOCALSTACK=false
fi

echo "=========================================="
echo "  Benchmark 08: S3 Sync - All Methods"
echo "=========================================="
echo ""
echo "Mode: Full S3 Passthrough"
echo "  - Sync: realtime (immediate S3 sync on write)"
echo "  - Read: s3 (read-through from S3)"
echo "  - Metadata: s3 (HEAD request on open)"
echo ""

# Verify required tools
for cmd in docker s3fs bc aws; do
    if ! command -v $cmd &> /dev/null; then
        echo "[ERROR] $cmd not found"
        exit 1
    fi
done

# Verify files exist
if [ ! -f "$WASM_DIR/bench-08-app.wasm" ]; then
    echo "[ERROR] WASM file not found: $WASM_DIR/bench-08-app.wasm"
    exit 1
fi

if [ ! -f "$BIN_DIR/bench-runtime-08" ]; then
    echo "[ERROR] Runtime binary not found: $BIN_DIR/bench-runtime-08"
    exit 1
fi

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    if mountpoint -q "$S3FS_MOUNT" 2>/dev/null; then
        fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
    fi
    rm -rf "$S3FS_MOUNT"
    # Kill any background RPC server
    pkill -f "vfs-rpc-server" 2>/dev/null || true
    if [ "$USE_LOCALSTACK" = true ]; then
        cd "$COMPOSE_DIR" && docker compose down 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Setup S3/GCS backend
if [ "$USE_LOCALSTACK" = true ]; then
    echo "=== Starting LocalStack ==="
    cd "$COMPOSE_DIR"
    docker compose up -d localstack

    echo "Waiting for LocalStack to be ready..."
    for i in {1..30}; do
        if curl -s http://localhost:4566/_localstack/health | grep -q '"s3".*"available"'; then
            echo "LocalStack is ready"
            break
        fi
        if [ $i -eq 30 ]; then
            echo "[ERROR] LocalStack did not become ready in time"
            exit 1
        fi
        sleep 1
    done

    # Create bucket
    aws --endpoint-url=http://localhost:4566 s3 mb s3://halycon-bench-08 2>/dev/null || true

    # Configure AWS CLI for LocalStack
    export AWS_ACCESS_KEY_ID=test
    export AWS_SECRET_ACCESS_KEY=test
    export AWS_DEFAULT_REGION=ap-northeast-1
    export AWS_ENDPOINT_URL=http://localhost:4566
    S3FS_URL="http://s3.localhost.localstack.cloud:4566"
    S3_BUCKET="halycon-bench-08"
    VFS_S3_BUCKET="halycon-bench-08"
    VFS_S3_PREFIX="vfs/"
else
    echo "=== Using real S3/GCS ==="
    echo "Bucket: $VFS_S3_BUCKET"
    echo "Endpoint: $AWS_ENDPOINT_URL"
    S3FS_URL="$AWS_ENDPOINT_URL"
    S3_BUCKET="$VFS_S3_BUCKET"
fi

# Common S3 passthrough settings
export VFS_SYNC_MODE=realtime
export VFS_READ_MODE=s3
export VFS_METADATA_MODE=s3

# ==========================================
#   Phase 1: s3fs-fuse Benchmark
# ==========================================
echo ""
echo "=========================================="
echo "  Phase 1: s3fs-fuse benchmark"
echo "=========================================="
echo ""

# Setup s3fs credentials
echo "=== Setting up s3fs ==="
if mountpoint -q "$S3FS_MOUNT" 2>/dev/null; then
    echo "Unmounting existing s3fs mount..."
    fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
    sleep 1
fi
rm -rf "$S3FS_MOUNT"

echo "${AWS_ACCESS_KEY_ID}:${AWS_SECRET_ACCESS_KEY}" > ~/.passwd-s3fs
chmod 600 ~/.passwd-s3fs

mkdir -p "$S3FS_MOUNT"
if [ "$USE_LOCALSTACK" = true ]; then
    s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
        -o passwd_file=~/.passwd-s3fs \
        -o url="$S3FS_URL" \
        -o use_path_request_style \
        -o allow_other 2>/dev/null || \
    s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
        -o passwd_file=~/.passwd-s3fs \
        -o url="$S3FS_URL" \
        -o use_path_request_style
else
    s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
        -o passwd_file=~/.passwd-s3fs \
        -o url="$S3FS_URL" \
        -o sigv4 \
        -o allow_other 2>/dev/null || \
    s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
        -o passwd_file=~/.passwd-s3fs \
        -o url="$S3FS_URL" \
        -o sigv4
fi

sleep 2
echo "s3fs mounted at $S3FS_MOUNT"

# Run s3fs benchmark
S3FS_START=$(date +%s%N)
BENCH_METHOD=s3fs ~/.wasmtime/bin/wasmtime run \
    --dir="${S3FS_MOUNT}::/data" \
    "$WASM_DIR/bench-08-app.wasm" 2>&1 | tee /tmp/s3fs-bench.log
S3FS_END=$(date +%s%N)
S3FS_TOTAL_MS=$(echo "scale=3; ($S3FS_END - $S3FS_START) / 1000000" | bc)
echo "[SYNC] s3fs total: ${S3FS_TOTAL_MS}ms"

# Unmount s3fs
fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
sleep 1

# ==========================================
#   Phase 2: vfs-host Benchmark
# ==========================================
echo ""
echo "=========================================="
echo "  Phase 2: vfs-host benchmark"
echo "=========================================="
echo ""

# Clear S3 data for fair comparison
if [ "$USE_LOCALSTACK" = true ]; then
    aws --endpoint-url=http://localhost:4566 s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
else
    aws s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
fi
sleep 1

VFSHOST_START=$(date +%s%N)
VFS_S3_BUCKET="$VFS_S3_BUCKET" \
VFS_S3_PREFIX="$VFS_S3_PREFIX" \
VFS_SYNC_MODE=realtime \
VFS_READ_MODE=s3 \
VFS_METADATA_MODE=s3 \
AWS_ENDPOINT_URL="${AWS_ENDPOINT_URL:-http://localhost:4566}" \
AWS_REGION="${AWS_REGION:-ap-northeast-1}" \
AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-test}" \
AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-test}" \
BENCH_METHOD=vfs-host \
"$BIN_DIR/bench-runtime-08" "$WASM_DIR/bench-08-app.wasm" 2>&1 | tee /tmp/vfshost-bench.log
VFSHOST_END=$(date +%s%N)
VFSHOST_TOTAL_MS=$(echo "scale=3; ($VFSHOST_END - $VFSHOST_START) / 1000000" | bc)
echo "[SYNC] vfs-host total: ${VFSHOST_TOTAL_MS}ms"

# ==========================================
#   Phase 3: wac-plug (vfs-adapter) Benchmark
# ==========================================
echo ""
echo "=========================================="
echo "  Phase 3: wac-plug benchmark"
echo "=========================================="
echo ""

# Clear S3 data for fair comparison
if [ "$USE_LOCALSTACK" = true ]; then
    aws --endpoint-url=http://localhost:4566 s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
else
    aws s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
fi
sleep 1

# Check if wac-composed WASM exists
if [ ! -f "$WASM_DIR/bench-08-wac.wasm" ]; then
    echo "[ERROR] WAC-composed WASM not found: $WASM_DIR/bench-08-wac.wasm"
    echo "[INFO] Skipping wac-plug benchmark"
    WAC_TOTAL_MS="N/A"
else
    WAC_START=$(date +%s%N)
    ~/.wasmtime/bin/wasmtime run \
        -S http \
        --env VFS_S3_BUCKET="$VFS_S3_BUCKET" \
        --env VFS_S3_PREFIX="$VFS_S3_PREFIX" \
        --env VFS_SYNC_MODE=realtime \
        --env VFS_READ_MODE=s3 \
        --env VFS_METADATA_MODE=s3 \
        --env AWS_ENDPOINT_URL="${AWS_ENDPOINT_URL:-http://localhost:4566}" \
        --env AWS_REGION="${AWS_REGION:-ap-northeast-1}" \
        --env AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-test}" \
        --env AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-test}" \
        --env BENCH_METHOD=wac-plug \
        "$WASM_DIR/bench-08-wac.wasm" 2>&1 | tee /tmp/wac-bench.log
    WAC_END=$(date +%s%N)
    WAC_TOTAL_MS=$(echo "scale=3; ($WAC_END - $WAC_START) / 1000000" | bc)
    echo "[SYNC] wac-plug total: ${WAC_TOTAL_MS}ms"
fi

# ==========================================
#   Phase 4: RPC (vfs-rpc-server) Benchmark
# ==========================================
echo ""
echo "=========================================="
echo "  Phase 4: RPC benchmark"
echo "=========================================="
echo ""

# Clear S3 data for fair comparison
if [ "$USE_LOCALSTACK" = true ]; then
    aws --endpoint-url=http://localhost:4566 s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
else
    aws s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
fi
sleep 1

# Check if RPC components exist
if [ ! -f "$WASM_DIR/vfs-rpc-server.wasm" ] || [ ! -f "$WASM_DIR/bench-08-rpc.wasm" ]; then
    echo "[ERROR] RPC components not found"
    echo "[INFO] Skipping RPC benchmark"
    RPC_TOTAL_MS="N/A"
else
    # Start RPC server in background
    echo "Starting RPC server..."
    ~/.wasmtime/bin/wasmtime run \
        -S inherit-network=y \
        -S http \
        --env VFS_S3_BUCKET="$VFS_S3_BUCKET" \
        --env VFS_S3_PREFIX="$VFS_S3_PREFIX" \
        --env VFS_SYNC_MODE=realtime \
        --env VFS_READ_MODE=s3 \
        --env VFS_METADATA_MODE=s3 \
        --env AWS_ENDPOINT_URL="${AWS_ENDPOINT_URL:-http://localhost:4566}" \
        --env AWS_REGION="${AWS_REGION:-ap-northeast-1}" \
        --env AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-test}" \
        --env AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-test}" \
        "$WASM_DIR/vfs-rpc-server.wasm" &
    RPC_SERVER_PID=$!
    sleep 2

    # Verify server is running
    if ! kill -0 $RPC_SERVER_PID 2>/dev/null; then
        echo "[ERROR] RPC server failed to start"
        RPC_TOTAL_MS="N/A"
    else
        RPC_START=$(date +%s%N)
        ~/.wasmtime/bin/wasmtime run \
            -S inherit-network=y \
            --env BENCH_METHOD=rpc \
            "$WASM_DIR/bench-08-rpc.wasm" 2>&1 | tee /tmp/rpc-bench.log
        RPC_END=$(date +%s%N)
        RPC_TOTAL_MS=$(echo "scale=3; ($RPC_END - $RPC_START) / 1000000" | bc)
        echo "[SYNC] RPC total: ${RPC_TOTAL_MS}ms"

        # Stop RPC server
        kill $RPC_SERVER_PID 2>/dev/null || true
    fi
fi

# ==========================================
#   Results Summary
# ==========================================
echo ""
echo "=========================================="
echo "  Results Summary"
echo "=========================================="
echo ""
echo "Mode: S3 Passthrough (realtime + read-through + metadata sync)"
echo ""
echo "Format: method,operation,size,time_ms,throughput_mb_s"
echo ""

echo "--- s3fs-fuse ---"
grep "^\[RESULT\]" /tmp/s3fs-bench.log | sed 's/\[RESULT\] //' || true
echo ""

echo "--- vfs-host ---"
grep "^\[RESULT\]" /tmp/vfshost-bench.log | sed 's/\[RESULT\] //' || true
echo ""

if [ -f /tmp/wac-bench.log ]; then
    echo "--- wac-plug ---"
    grep "^\[RESULT\]" /tmp/wac-bench.log | sed 's/\[RESULT\] //' || true
    echo ""
fi

if [ -f /tmp/rpc-bench.log ]; then
    echo "--- RPC ---"
    grep "^\[RESULT\]" /tmp/rpc-bench.log | sed 's/\[RESULT\] //' || true
    echo ""
fi

echo "=========================================="
echo "  Total Time Comparison (E2E)"
echo "=========================================="
echo ""
echo "s3fs-fuse:  ${S3FS_TOTAL_MS}ms"
echo "vfs-host:   ${VFSHOST_TOTAL_MS}ms"
echo "wac-plug:   ${WAC_TOTAL_MS}ms"
echo "RPC:        ${RPC_TOTAL_MS}ms"
echo ""

echo "=========================================="
echo "  Notes"
echo "=========================================="
echo ""
echo "All methods run in S3 passthrough mode:"
echo "  - Write: immediate S3 PUT after each write"
echo "  - Read: GET from S3 on every read (read-through)"
echo "  - Metadata: HEAD request on every open"
echo ""

echo "=== Benchmark 08 Complete ==="
