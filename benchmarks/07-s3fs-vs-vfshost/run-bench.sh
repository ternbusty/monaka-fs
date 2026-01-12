#!/bin/bash
# Benchmark 07: s3fs-fuse vs vfs-host S3 Sync
#
# This benchmark compares:
# - s3fs-fuse: FUSE-based S3 mount (synchronous S3 I/O)
# - vfs-host: In-memory VFS with background S3 sync (deferred persistence)
#
# Prerequisites:
# - LocalStack running on localhost:4566
# - s3fs-fuse installed (Linux only)
# - Rust toolchain with wasm32-wasip2 target
#
# Usage:
#   ./run-bench.sh          # Run vfs-host benchmark only
#   ./run-bench.sh --all    # Run both s3fs and vfs-host (Linux only)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# S3 configuration
S3_BUCKET="halycon-bench-07"
S3_PREFIX="vfs/"
S3_ENDPOINT="http://localhost:4566"
S3_MOUNT_DIR="/tmp/s3fs-bench"

echo "=========================================="
echo "  Benchmark 07: s3fs vs vfs-host"
echo "=========================================="
echo ""

# ==========================================
#   Build Components
# ==========================================
echo "=== Building benchmark WASM app ==="
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2

BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-s3fs-vs-vfshost.wasm"

echo ""
echo "=== Building bench-runtime ==="
cd "$SCRIPT_DIR/bench-runtime"
cargo build --release

BENCH_RUNTIME="$SCRIPT_DIR/bench-runtime/target/release/bench-runtime-s3fs-vs-vfshost"

# ==========================================
#   Setup LocalStack
# ==========================================
echo ""
echo "=== Checking LocalStack ==="

if ! curl -s http://localhost:4566/_localstack/health | grep -q '"s3"'; then
    echo "[ERROR] LocalStack is not running. Please start it first:"
    echo "  docker run -d -p 4566:4566 localstack/localstack"
    exit 1
fi
echo "LocalStack is running"

# Create bucket if not exists
if ! curl -s -o /dev/null -w "%{http_code}" "http://localhost:4566/$S3_BUCKET" 2>/dev/null | grep -q "200"; then
    echo "Creating S3 bucket: $S3_BUCKET"
    curl -s -X PUT "http://localhost:4566/$S3_BUCKET" > /dev/null
fi

# Clear existing data
echo "Clearing existing data in S3..."
curl -s "http://localhost:4566/$S3_BUCKET?prefix=${S3_PREFIX}" | \
    sed 's/<Key>/\n<Key>/g' | grep '<Key>' | sed 's/<Key>\([^<]*\)<.*/\1/' | \
    while read key; do
        if [ -n "$key" ]; then
            curl -s -X DELETE "http://localhost:4566/$S3_BUCKET/$key" > /dev/null
        fi
    done

echo ""

# ==========================================
#   Run s3fs Benchmark (Linux only)
# ==========================================
if [ "$1" = "--all" ]; then
    echo "=========================================="
    echo "  Running: s3fs-fuse Benchmark"
    echo "=========================================="
    echo ""

    if ! command -v s3fs &> /dev/null; then
        echo "[ERROR] s3fs-fuse is not installed"
        echo "Install with: sudo apt-get install s3fs"
        exit 1
    fi

    # Setup s3fs mount
    mkdir -p "$S3_MOUNT_DIR"

    # Create credentials file
    echo "test:test" > /tmp/s3fs-passwd
    chmod 600 /tmp/s3fs-passwd

    # Mount s3fs
    echo "Mounting s3fs..."
    s3fs "$S3_BUCKET" "$S3_MOUNT_DIR" \
        -o passwd_file=/tmp/s3fs-passwd \
        -o url="$S3_ENDPOINT" \
        -o use_path_request_style \
        -o allow_other \
        -o umask=0000

    # Run native benchmark (simulating what WASM does)
    echo "Running s3fs benchmark..."

    S3FS_START=$(date +%s%N)

    # Create data directory
    mkdir -p "$S3_MOUNT_DIR/data"

    # Run benchmark iterations
    for size in 1024 10240 102400 1048576 10485760; do
        case $size in
            1024) label="1KB" ;;
            10240) label="10KB" ;;
            102400) label="100KB" ;;
            1048576) label="1MB" ;;
            10485760) label="10MB" ;;
        esac

        echo "--- File Size: $label ---"

        # Sequential write
        write_times=()
        for i in {0..4}; do
            start=$(date +%s%N)
            dd if=/dev/zero of="$S3_MOUNT_DIR/data/benchmark_write_${label}_${i}.dat" bs=$size count=1 2>/dev/null
            end=$(date +%s%N)
            write_times+=( $((($end - $start) / 1000000)) )
            rm -f "$S3_MOUNT_DIR/data/benchmark_write_${label}_${i}.dat"
        done
        write_median=$(echo "${write_times[@]}" | tr ' ' '\n' | sort -n | head -3 | tail -1)
        write_throughput=$(echo "scale=2; ($size / 1048576.0) / ($write_median / 1000.0)" | bc -l 2>/dev/null || echo "N/A")
        echo "[RESULT] seq_write,$label,$write_median,$write_throughput"

        # Sequential read (create files first)
        for i in {0..4}; do
            dd if=/dev/zero of="$S3_MOUNT_DIR/data/benchmark_read_${label}_${i}.dat" bs=$size count=1 2>/dev/null
        done
        read_times=()
        for i in {0..4}; do
            start=$(date +%s%N)
            cat "$S3_MOUNT_DIR/data/benchmark_read_${label}_${i}.dat" > /dev/null
            end=$(date +%s%N)
            read_times+=( $((($end - $start) / 1000000)) )
            rm -f "$S3_MOUNT_DIR/data/benchmark_read_${label}_${i}.dat"
        done
        read_median=$(echo "${read_times[@]}" | tr ' ' '\n' | sort -n | head -3 | tail -1)
        read_throughput=$(echo "scale=2; ($size / 1048576.0) / ($read_median / 1000.0)" | bc -l 2>/dev/null || echo "N/A")
        echo "[RESULT] seq_read,$label,$read_median,$read_throughput"
        echo ""
    done

    S3FS_END=$(date +%s%N)
    S3FS_TOTAL=$(echo "scale=3; ($S3FS_END - $S3FS_START) / 1000000" | bc)

    echo "[TOTAL] s3fs benchmark: ${S3FS_TOTAL}ms"

    # Unmount s3fs
    echo "Unmounting s3fs..."
    fusermount -u "$S3_MOUNT_DIR" 2>/dev/null || umount "$S3_MOUNT_DIR" 2>/dev/null || true
    rm -f /tmp/s3fs-passwd

    # Clear S3 data
    echo "Clearing S3 data..."
    curl -s "http://localhost:4566/$S3_BUCKET?prefix=" | \
        sed 's/<Key>/\n<Key>/g' | grep '<Key>' | sed 's/<Key>\([^<]*\)<.*/\1/' | \
        while read key; do
            if [ -n "$key" ]; then
                curl -s -X DELETE "http://localhost:4566/$S3_BUCKET/$key" > /dev/null
            fi
        done

    echo ""
fi

# ==========================================
#   Run vfs-host Benchmark
# ==========================================
echo "=========================================="
echo "  Running: vfs-host S3 Sync Benchmark"
echo "=========================================="
echo ""

VFSHOST_START=$(date +%s%N)

VFS_S3_BUCKET="$S3_BUCKET" \
VFS_S3_PREFIX="$S3_PREFIX" \
VFS_SYNC_MODE=batch \
AWS_ENDPOINT_URL="$S3_ENDPOINT" \
AWS_REGION=ap-northeast-1 \
AWS_ACCESS_KEY_ID=test \
AWS_SECRET_ACCESS_KEY=test \
BENCH_MODE=vfs-host \
"$BENCH_RUNTIME" "$BENCH_WASM" 2>&1 | tee /tmp/vfshost-bench.log

VFSHOST_END=$(date +%s%N)
VFSHOST_TOTAL=$(echo "scale=3; ($VFSHOST_END - $VFSHOST_START) / 1000000" | bc)

echo ""
echo "[TOTAL] vfs-host benchmark: ${VFSHOST_TOTAL}ms"

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

if [ "$1" = "--all" ]; then
    echo "--- s3fs-fuse Results ---"
    echo "(See output above)"
    echo ""
fi

echo "--- vfs-host S3 Sync Results ---"
grep "^\[RESULT\]" /tmp/vfshost-bench.log | sed 's/\[RESULT\] //' || true

echo ""
echo "Note:"
echo "  s3fs: Each I/O operation involves S3 API call (synchronous)"
echo "  vfs-host: I/O is in-memory, S3 sync happens in background"
echo ""
echo "=== Benchmark 07 Complete ==="
