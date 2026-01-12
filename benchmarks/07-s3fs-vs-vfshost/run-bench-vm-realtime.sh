#!/bin/bash
# Benchmark 07: s3fs-fuse vs vfs-host S3 Sync (VM version) - REALTIME MODE
# Measures end-to-end time: write start → S3 arrival
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
echo "  Benchmark 07: s3fs vs vfs-host (REALTIME)"
echo "=========================================="
echo ""
echo "Measuring: write start → S3 arrival"
echo "Sync Mode: REALTIME (immediate S3 sync)"
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
    # For real S3/GCS
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

# ==========================================
#   s3fs Benchmark
# ==========================================
echo ""
echo "=========================================="
echo "  Running: s3fs-fuse benchmark"
echo "=========================================="
echo ""

S3FS_RESULTS=""
S3FS_TOTAL_START=$(date +%s%N)

# Test sizes
for size_kb in 1 10 100 1024 10240; do
    if [ $size_kb -lt 1024 ]; then
        label="${size_kb}KB"
        size_bytes=$((size_kb * 1024))
        dd_count=$size_kb
        dd_bs="1K"
    else
        size_mb=$((size_kb / 1024))
        label="${size_mb}MB"
        size_bytes=$((size_kb * 1024))
        dd_count=$size_mb
        dd_bs="1M"
    fi

    file="$S3FS_MOUNT/benchmark_${label}.dat"

    echo "--- File Size: $label ---"

    # Sequential Write (end-to-end: write + sync to S3)
    write_times=()
    for i in $(seq 1 $ITERATIONS); do
        rm -f "$file" 2>/dev/null || true

        start_ns=$(date +%s%N)
        timeout 120 dd if=/dev/urandom of="$file" bs=$dd_bs count=$dd_count 2>/dev/null || true
        sync || true
        end_ns=$(date +%s%N)

        duration_ms=$(echo "scale=3; ($end_ns - $start_ns) / 1000000" | bc)
        write_times+=($duration_ms)
        sleep 1
    done

    write_median=$(printf '%s\n' "${write_times[@]}" | sort -n | sed -n "$((($ITERATIONS + 1) / 2))p")
    write_throughput=$(echo "scale=2; ($size_bytes / 1048576) / ($write_median / 1000)" | bc)
    echo "[RESULT] s3fs_write,$label,$write_median,$write_throughput"
    S3FS_RESULTS="$S3FS_RESULTS
[RESULT] s3fs_write,$label,$write_median,$write_throughput"

    # Sequential Read
    read_times=()
    for i in $(seq 1 $ITERATIONS); do
        echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null 2>&1 || true

        start_ns=$(date +%s%N)
        timeout 120 cat "$file" > /dev/null || true
        end_ns=$(date +%s%N)

        duration_ms=$(echo "scale=3; ($end_ns - $start_ns) / 1000000" | bc)
        read_times+=($duration_ms)
        sleep 1
    done

    read_median=$(printf '%s\n' "${read_times[@]}" | sort -n | sed -n "$((($ITERATIONS + 1) / 2))p")
    read_throughput=$(echo "scale=2; ($size_bytes / 1048576) / ($read_median / 1000)" | bc)
    echo "[RESULT] s3fs_read,$label,$read_median,$read_throughput"
    S3FS_RESULTS="$S3FS_RESULTS
[RESULT] s3fs_read,$label,$read_median,$read_throughput"

    rm -f "$file" 2>/dev/null || true
    echo ""
done

S3FS_TOTAL_END=$(date +%s%N)
S3FS_TOTAL_MS=$(echo "scale=3; ($S3FS_TOTAL_END - $S3FS_TOTAL_START) / 1000000" | bc)
echo "[SYNC] s3fs total (all ops): ${S3FS_TOTAL_MS}ms"

# Unmount s3fs
fusermount -u "$S3FS_MOUNT" 2>/dev/null || true

# ==========================================
#   vfs-host Benchmark (REALTIME mode)
# ==========================================
echo ""
echo "=========================================="
echo "  Running: vfs-host S3 Sync (REALTIME)"
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

echo "--- s3fs-fuse (E2E: write → S3) ---"
echo "$S3FS_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""

echo "--- vfs-host S3 Sync (REALTIME) ---"
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
echo "  s3fs: synchronous (write blocks until S3 confirms)"
echo "  vfs-host REALTIME: immediate S3 sync after each write"

echo ""
echo "=== Benchmark 07 Complete ==="
