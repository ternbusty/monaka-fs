#!/bin/bash
# Benchmark 04: s3fs-fuse vs Halycon S3 Sync - REALTIME MODE
# Measures end-to-end time: write start → S3 arrival
#
# This script runs on a Linux VM with s3fs installed
# Uses VFS_SYNC_MODE=realtime for immediate S3 sync after each write

set -e

BENCH_DIR="/home/ubuntu/halycon-bench"
WASM_DIR="$BENCH_DIR/wasm"
COMPOSE_DIR="$BENCH_DIR/04"
S3FS_MOUNT="/tmp/s3fs-bench"
SERVER_LOG="/tmp/vfs-rpc-server.log"
ITERATIONS=5
ENV_FILE="$COMPOSE_DIR/.env"

export PATH="$HOME/.wasmtime/bin:$PATH"

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
echo "  Benchmark 04: s3fs vs Halycon REALTIME"
echo "=========================================="
echo ""
echo "Measuring: write start → S3 arrival"
echo ""

# Verify required tools
for cmd in wasmtime docker s3fs bc aws; do
    if ! command -v $cmd &> /dev/null; then
        echo "[ERROR] $cmd not found. Run vm-setup.sh first."
        exit 1
    fi
done

# Verify WASM files exist
for wasm in bench-04-rpc.wasm vfs_rpc_server.wasm; do
    if [ ! -f "$WASM_DIR/$wasm" ]; then
        echo "[ERROR] $WASM_DIR/$wasm not found"
        exit 1
    fi
done

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    if mountpoint -q "$S3FS_MOUNT" 2>/dev/null; then
        fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
    fi
    rm -rf "$S3FS_MOUNT"
    if [ -n "$SERVER_PID" ] && kill -0 $SERVER_PID 2>/dev/null; then
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
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
        if curl -s http://localhost:4566/_localstack/health | grep -q '"s3".*"running"'; then
            echo "LocalStack is ready"
            break
        fi
        if [ $i -eq 30 ]; then
            echo "[ERROR] LocalStack did not become ready in time"
            exit 1
        fi
        sleep 1
    done

    # Configure AWS CLI for LocalStack
    export AWS_ACCESS_KEY_ID=test
    export AWS_SECRET_ACCESS_KEY=test
    export AWS_DEFAULT_REGION=ap-northeast-1
    export AWS_ENDPOINT_URL=http://localhost:4566
    S3FS_URL="http://s3.localhost.localstack.cloud:4566"
    S3_BUCKET="halycon-bench"
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
    # For GCS, need different options
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

# Test sizes in MB
for size_mb in 1 10 100; do
    size_bytes=$((size_mb * 1024 * 1024))
    label="${size_mb}MB"
    file="$S3FS_MOUNT/benchmark_${size_mb}mb.dat"

    echo "--- File Size: $label ---"

    # Sequential Write (end-to-end: write + sync to S3)
    write_times=()
    for i in $(seq 1 $ITERATIONS); do
        # Clear any existing file
        rm -f "$file" 2>/dev/null || true

        start_ns=$(date +%s%N)
        dd if=/dev/urandom of="$file" bs=1M count=$size_mb 2>/dev/null
        sync
        end_ns=$(date +%s%N)

        duration_ms=$(echo "scale=3; ($end_ns - $start_ns) / 1000000" | bc)
        write_times+=($duration_ms)
    done

    # Calculate median
    write_median=$(printf '%s\n' "${write_times[@]}" | sort -n | sed -n "$((($ITERATIONS + 1) / 2))p")
    write_throughput=$(echo "scale=2; $size_mb / ($write_median / 1000)" | bc)
    echo "[RESULT] s3fs_write,$label,$write_median,$write_throughput"
    S3FS_RESULTS="$S3FS_RESULTS
[RESULT] s3fs_write,$label,$write_median,$write_throughput"

    # Sequential Read
    read_times=()
    for i in $(seq 1 $ITERATIONS); do
        echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null 2>&1 || true

        start_ns=$(date +%s%N)
        cat "$file" > /dev/null
        end_ns=$(date +%s%N)

        duration_ms=$(echo "scale=3; ($end_ns - $start_ns) / 1000000" | bc)
        read_times+=($duration_ms)
    done

    read_median=$(printf '%s\n' "${read_times[@]}" | sort -n | sed -n "$((($ITERATIONS + 1) / 2))p")
    read_throughput=$(echo "scale=2; $size_mb / ($read_median / 1000)" | bc)
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
fusermount -u "$S3FS_MOUNT"

# ==========================================
#   Halycon Benchmark
# ==========================================
echo ""
echo "=========================================="
echo "  Running: Halycon REALTIME Sync"
echo "=========================================="
echo ""

# Clear existing S3 data
if [ "$USE_LOCALSTACK" = true ]; then
    aws s3 rm s3://halycon-bench/vfs/files/ --recursive 2>/dev/null || true
else
    # For GCS, use gsutil to clear existing data
    export PATH="$PATH:$HOME/google-cloud-sdk/bin"

    # Create boto config for gsutil with HMAC credentials
    cat > ~/.boto << EOF
[Credentials]
gs_access_key_id = ${AWS_ACCESS_KEY_ID}
gs_secret_access_key = ${AWS_SECRET_ACCESS_KEY}

[GSUtil]
prefer_api = xml
EOF

    echo "Clearing existing GCS data..."
    gsutil -m rm -r "gs://${VFS_S3_BUCKET}/${VFS_S3_PREFIX}**" 2>/dev/null || true
fi

echo "Starting vfs-rpc-server (REALTIME mode)..."
wasmtime run -S inherit-network=y -S http \
    --env VFS_S3_BUCKET="${VFS_S3_BUCKET:-halycon-bench}" \
    --env VFS_S3_PREFIX="${VFS_S3_PREFIX:-vfs/}" \
    --env AWS_ENDPOINT_URL="${AWS_ENDPOINT_URL:-http://localhost:4566}" \
    --env AWS_ACCESS_KEY_ID="$AWS_ACCESS_KEY_ID" \
    --env AWS_SECRET_ACCESS_KEY="$AWS_SECRET_ACCESS_KEY" \
    --env AWS_REGION="${AWS_REGION:-ap-northeast-1}" \
    --env VFS_SYNC_MODE=realtime \
    "$WASM_DIR/vfs_rpc_server.wasm" > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!

sleep 3
if ! kill -0 $SERVER_PID 2>/dev/null; then
    echo "[ERROR] Server failed to start"
    cat "$SERVER_LOG"
    exit 1
fi

TOTAL_START=$(date +%s%N)
HALYCON_OUTPUT=$(wasmtime run -S inherit-network=y "$WASM_DIR/bench-04-rpc.wasm" 2>&1)
echo "$HALYCON_OUTPUT"

# Wait for S3 sync
echo ""
echo "Waiting for S3 sync..."
S3_PREFIX="${VFS_S3_PREFIX:-vfs/}"
if [ "$USE_LOCALSTACK" = true ]; then
    # Poll for all 3 files using aws CLI
    while true; do
        size_1mb=$(aws s3api head-object --bucket halycon-bench --key "${S3_PREFIX}files/data/benchmark_1MB.dat" 2>/dev/null | jq -r '.ContentLength // 0')
        size_10mb=$(aws s3api head-object --bucket halycon-bench --key "${S3_PREFIX}files/data/benchmark_10MB.dat" 2>/dev/null | jq -r '.ContentLength // 0')
        size_100mb=$(aws s3api head-object --bucket halycon-bench --key "${S3_PREFIX}files/data/benchmark_100MB.dat" 2>/dev/null | jq -r '.ContentLength // 0')

        # Check all files exist with expected sizes
        if [ "${size_1mb:-0}" -gt 1000000 ] && [ "${size_10mb:-0}" -gt 10000000 ] && [ "${size_100mb:-0}" -gt 100000000 ]; then
            break
        fi
        sleep 0.05
    done
else
    # For GCS, configure gsutil with HMAC credentials and poll
    export PATH="$PATH:$HOME/google-cloud-sdk/bin"

    # Create boto config for gsutil with HMAC credentials
    cat > ~/.boto << EOF
[Credentials]
gs_access_key_id = ${AWS_ACCESS_KEY_ID}
gs_secret_access_key = ${AWS_SECRET_ACCESS_KEY}

[GSUtil]
prefer_api = xml
EOF

    echo "Polling GCS for sync completion..."
    while true; do
        # Check if all 3 benchmark files exist with correct sizes
        size_1mb=$(gsutil ls -l "gs://${VFS_S3_BUCKET}/${S3_PREFIX}files/data/benchmark_1MB.dat" 2>/dev/null | awk 'NR==1{print $1}' || echo "0")
        size_10mb=$(gsutil ls -l "gs://${VFS_S3_BUCKET}/${S3_PREFIX}files/data/benchmark_10MB.dat" 2>/dev/null | awk 'NR==1{print $1}' || echo "0")
        size_100mb=$(gsutil ls -l "gs://${VFS_S3_BUCKET}/${S3_PREFIX}files/data/benchmark_100MB.dat" 2>/dev/null | awk 'NR==1{print $1}' || echo "0")

        # Check all files exist with expected sizes
        if [ "${size_1mb:-0}" -gt 1000000 ] && [ "${size_10mb:-0}" -gt 10000000 ] && [ "${size_100mb:-0}" -gt 100000000 ]; then
            break
        fi
        sleep 0.1
    done
fi

TOTAL_END=$(date +%s%N)
TOTAL_MS=$(echo "scale=3; ($TOTAL_END - $TOTAL_START) / 1000000" | bc)

kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
SERVER_PID=""

echo "[SYNC] Halycon total (ops + S3 sync): ${TOTAL_MS}ms"

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

echo "--- Halycon REALTIME Sync ---"
echo "$HALYCON_OUTPUT" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""

echo "=========================================="
echo "  Total Time Comparison (E2E)"
echo "=========================================="
echo ""
echo "s3fs total:    ${S3FS_TOTAL_MS}ms"
echo "Halycon total: ${TOTAL_MS}ms"
echo ""

echo "Note: Both totals include write + S3 arrival time."
echo "      s3fs: synchronous (write blocks until S3 confirms)"
echo "      Halycon REALTIME: immediate sync after each write"

echo ""
echo "=== Benchmark 04 Complete ==="
