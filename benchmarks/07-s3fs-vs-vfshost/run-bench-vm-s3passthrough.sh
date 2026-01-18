#!/bin/bash
# Benchmark 07: s3fs-fuse vs vfs-host S3 Sync (VM version) - S3 PASSTHROUGH MODE
# Full S3 passthrough: all reads/writes/metadata go through S3 (like s3fs-fuse)
#
# This script runs on a Linux VM with s3fs installed
# Supports both LocalStack (default) and real S3/GCS via .env file

set -e

BENCH_DIR="/home/ubuntu/halycon-bench"
WASM_DIR="$BENCH_DIR/wasm"
BIN_DIR="$BENCH_DIR/bin"
COMPOSE_DIR="$BENCH_DIR/07"
S3FS_MOUNT="/tmp/s3fs-bench"
ITERATIONS=5
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
echo "  Benchmark 07: s3fs vs vfs-host (S3 PASSTHROUGH)"
echo "=========================================="
echo ""
echo "Full S3 passthrough mode (fair comparison with s3fs-fuse)"
echo "Sync Mode: REALTIME (immediate S3 sync on write)"
echo "Read Mode: S3 (read-through from S3)"
echo "Metadata Mode: S3 (HEAD request on every open)"
echo ""

# Verify required tools
for cmd in docker s3fs bc aws; do
    if ! command -v $cmd &> /dev/null; then
        echo "[ERROR] $cmd not found"
        exit 1
    fi
done

# Verify files exist
if [ ! -f "$WASM_DIR/bench-s3fs-vs-vfshost.wasm" ]; then
    echo "[ERROR] WASM file not found: $WASM_DIR/bench-s3fs-vs-vfshost.wasm"
    exit 1
fi

if [ ! -f "$BIN_DIR/bench-runtime-s3fs-vs-vfshost" ]; then
    echo "[ERROR] Runtime binary not found: $BIN_DIR/bench-runtime-s3fs-vs-vfshost"
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
    aws --endpoint-url=http://localhost:4566 s3 mb s3://halycon-bench-07 2>/dev/null || true

    # Configure AWS CLI for LocalStack
    export AWS_ACCESS_KEY_ID=test
    export AWS_SECRET_ACCESS_KEY=test
    export AWS_DEFAULT_REGION=ap-northeast-1
    export AWS_ENDPOINT_URL=http://localhost:4566
    S3FS_URL="http://s3.localhost.localstack.cloud:4566"
    S3_BUCKET="halycon-bench-07"
    VFS_S3_BUCKET="halycon-bench-07"
    VFS_S3_PREFIX="vfs/"
else
    echo "=== Using real S3/GCS ==="
    echo "Bucket: $VFS_S3_BUCKET"
    echo "Endpoint: $AWS_ENDPOINT_URL"
    S3FS_URL="$AWS_ENDPOINT_URL"
    S3_BUCKET="$VFS_S3_BUCKET"
fi

# Setup s3fs credentials
echo ""
echo "=== Setting up s3fs ==="

# Cleanup any existing mount first
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
    # For real S3/GCS (with debug logging)
    s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
        -o passwd_file=~/.passwd-s3fs \
        -o url="$S3FS_URL" \
        -o sigv4 \
        -o dbglevel=info \
        -o curldbg \
        -o allow_other 2>/dev/null || \
    s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
        -o passwd_file=~/.passwd-s3fs \
        -o url="$S3FS_URL" \
        -o sigv4 \
        -o dbglevel=info \
        -o curldbg
fi

sleep 2
echo "s3fs mounted at $S3FS_MOUNT"

# ==========================================
#   s3fs Benchmark (using same WASM app)
# ==========================================
echo ""
echo "=========================================="
echo "  Running: s3fs-fuse benchmark (WASM)"
echo "=========================================="
echo ""

# Use wasmtime to run the same WASM app with s3fs mount as /data
S3FS_TOTAL_START=$(date +%s%N)

~/.wasmtime/bin/wasmtime run \
    --dir="${S3FS_MOUNT}::/data" \
    "$WASM_DIR/bench-s3fs-vs-vfshost.wasm" 2>&1 | tee /tmp/s3fs-bench.log

S3FS_TOTAL_END=$(date +%s%N)
S3FS_TOTAL_MS=$(echo "scale=3; ($S3FS_TOTAL_END - $S3FS_TOTAL_START) / 1000000" | bc)
echo "[SYNC] s3fs total (all ops): ${S3FS_TOTAL_MS}ms"

# Unmount s3fs
fusermount -u "$S3FS_MOUNT" 2>/dev/null || true

# ==========================================
#   vfs-host Benchmark (S3 PASSTHROUGH mode)
# ==========================================
echo ""
echo "=========================================="
echo "  Running: vfs-host S3 Sync (S3 PASSTHROUGH)"
echo "=========================================="
echo ""

# Ensure bucket exists and clear existing S3 data
sleep 2
if [ "$USE_LOCALSTACK" = true ]; then
    aws --endpoint-url=http://localhost:4566 s3 mb s3://$S3_BUCKET 2>/dev/null || true
    aws --endpoint-url=http://localhost:4566 s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
else
    aws s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
fi
sleep 1

TOTAL_START=$(date +%s%N)

VFS_S3_BUCKET="$VFS_S3_BUCKET" \
VFS_S3_PREFIX="$VFS_S3_PREFIX" \
VFS_SYNC_MODE=realtime \
VFS_READ_MODE=s3 \
VFS_METADATA_MODE=s3 \
AWS_ENDPOINT_URL="${AWS_ENDPOINT_URL:-http://localhost:4566}" \
AWS_REGION="${AWS_REGION:-ap-northeast-1}" \
AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-test}" \
AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-test}" \
"$BIN_DIR/bench-runtime-s3fs-vs-vfshost" "$WASM_DIR/bench-s3fs-vs-vfshost.wasm" 2>&1 | tee /tmp/vfshost-bench.log

TOTAL_END=$(date +%s%N)
TOTAL_MS=$(echo "scale=3; ($TOTAL_END - $TOTAL_START) / 1000000" | bc)

echo "[SYNC] vfs-host total (ops + S3 sync): ${TOTAL_MS}ms"

# ==========================================
#   Results Summary
# ==========================================
echo ""
echo "=========================================="
echo "  Results Summary"
echo "=========================================="
echo ""
echo "Format: method,operation,size,time_ms,throughput_mb_s"
echo ""

echo "--- s3fs-fuse (WASM via wasmtime) ---"
grep "^\[RESULT\]" /tmp/s3fs-bench.log | sed 's/\[RESULT\] //' || true
echo ""

echo "--- vfs-host S3 Sync (S3 PASSTHROUGH) ---"
grep "^\[RESULT\]" /tmp/vfshost-bench.log | sed 's/\[RESULT\] //' || true
echo ""

echo "=========================================="
echo "  Total Time Comparison (E2E)"
echo "=========================================="
echo ""
echo "s3fs total:     ${S3FS_TOTAL_MS}ms"
echo "vfs-host total: ${TOTAL_MS}ms"
echo ""

echo "Note: "
echo "  Both s3fs and vfs-host are running in S3 passthrough mode:"
echo "  - Write: immediate S3 PUT after each write"
echo "  - Read: GET from S3 on every read (read-through)"
echo "  - Metadata: HEAD request on every open"
echo "  This provides a fair apple-to-apple comparison."

echo ""
echo "=========================================="
echo "  S3 Request Count Analysis"
echo "=========================================="
echo ""

# Count s3fs S3 requests from syslog
echo "--- s3fs S3 Requests (from syslog) ---"
S3FS_PUT_COUNT=$(grep s3fs /var/log/syslog 2>/dev/null | grep -c "PUT" || echo "0")
S3FS_GET_COUNT=$(grep s3fs /var/log/syslog 2>/dev/null | grep -c "GET" || echo "0")
S3FS_DELETE_COUNT=$(grep s3fs /var/log/syslog 2>/dev/null | grep -c "DELETE" || echo "0")
echo "PUT requests:    $S3FS_PUT_COUNT"
echo "GET requests:    $S3FS_GET_COUNT"
echo "DELETE requests: $S3FS_DELETE_COUNT"
echo ""

# Count vfs-host S3 requests from log
echo "--- vfs-host S3 Requests (from log) ---"
VFS_UPLOAD_COUNT=$(grep -c "Uploaded:" /tmp/vfshost-bench.log 2>/dev/null || echo "0")
VFS_REFRESH_COUNT=$(grep -c "Refreshed from S3:" /tmp/vfshost-bench.log 2>/dev/null || echo "0")
VFS_DELETE_COUNT=$(grep -c "Deleted from S3:" /tmp/vfshost-bench.log 2>/dev/null || echo "0")
echo "Upload (PUT):    $VFS_UPLOAD_COUNT"
echo "Refresh (GET):   $VFS_REFRESH_COUNT"
echo "Delete:          $VFS_DELETE_COUNT"
echo ""

echo "=== Benchmark 07 Complete ==="
