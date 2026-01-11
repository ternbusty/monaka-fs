#!/bin/bash
# Benchmark 01: ext4 vs tmpfs vs VFS (VM version)
# This script runs on the Linux VM

set -e

# This benchmark requires Linux with tmpfs support
if [[ "$(uname)" == "Darwin" ]]; then
    echo "Error: This benchmark requires Linux with tmpfs support."
    echo "macOS does not have tmpfs - use run-bench-vm.sh to run in a Linux VM."
    exit 1
fi

BENCH_DIR="/home/ubuntu/halycon-bench"
WASM_DIR="$BENCH_DIR/wasm"

export PATH="$HOME/.wasmtime/bin:$PATH"

echo "=========================================="
echo "  Benchmark 01: ext4 vs tmpfs vs VFS"
echo "=========================================="
echo ""

# Verify wasmtime is available
if ! command -v wasmtime &> /dev/null; then
    echo "[ERROR] wasmtime not found. Run vm-setup.sh first."
    exit 1
fi

# Verify WASM files exist
if [ ! -f "$WASM_DIR/bench-01-raw.wasm" ]; then
    echo "[ERROR] $WASM_DIR/bench-01-raw.wasm not found"
    exit 1
fi

if [ ! -f "$WASM_DIR/bench-01-composed.wasm" ]; then
    echo "[ERROR] $WASM_DIR/bench-01-composed.wasm not found"
    exit 1
fi

# Helper function to create test files (5 files per size for cold read iterations)
create_test_files() {
    local dir="$1"
    echo "Creating test files in $dir..."
    for i in 0 1 2 3 4; do
        dd if=/dev/urandom of="$dir/1mb_${i}.dat" bs=1M count=1 2>/dev/null
        dd if=/dev/urandom of="$dir/10mb_${i}.dat" bs=1M count=10 2>/dev/null
        dd if=/dev/urandom of="$dir/100mb_${i}.dat" bs=1M count=100 2>/dev/null
    done
}

clear_cache() {
    echo "Clearing page cache..."
    sync
    sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches' 2>/dev/null || echo "[WARN] Could not clear cache (need sudo)"
}

# Setup directories
EXT4_DIR="$BENCH_DIR/testdata"
TMPFS_DIR="/dev/shm/halycon-bench"
mkdir -p "$EXT4_DIR" "$TMPFS_DIR"
trap "rm -rf $TMPFS_DIR $EXT4_DIR" EXIT

# ==========================================
# ext4 benchmark (read-only mode with cache clear)
# ==========================================
echo "=== Running: Host ext4 baseline ==="
echo "Using ext4 at $EXT4_DIR"
echo ""

create_test_files "$EXT4_DIR"
clear_cache

echo "--- Write benchmark ---"
EXT4_WRITE_RESULTS=$(wasmtime run --dir="$EXT4_DIR::/mnt" "$WASM_DIR/bench-01-raw.wasm" 2>&1)
echo "$EXT4_WRITE_RESULTS" | grep -E "seq_write"

clear_cache

echo "--- Read benchmark (cache cleared) ---"
EXT4_READ_RESULTS=$(wasmtime run --dir="$EXT4_DIR::/mnt" "$WASM_DIR/bench-01-raw.wasm" -- --read-only 2>&1)
echo "$EXT4_READ_RESULTS"

rm -f "$EXT4_DIR"/*.dat

# ==========================================
# tmpfs benchmark (read-only mode with cache clear)
# ==========================================
echo ""
echo "=== Running: Host tmpfs baseline ==="
echo "Using /dev/shm (tmpfs) at $TMPFS_DIR"
echo ""

create_test_files "$TMPFS_DIR"
clear_cache

echo "--- Write benchmark ---"
TMPFS_WRITE_RESULTS=$(wasmtime run --dir="$TMPFS_DIR::/mnt" "$WASM_DIR/bench-01-raw.wasm" 2>&1)
echo "$TMPFS_WRITE_RESULTS" | grep -E "seq_write"

clear_cache

echo "--- Read benchmark (cache cleared) ---"
TMPFS_READ_RESULTS=$(wasmtime run --dir="$TMPFS_DIR::/mnt" "$WASM_DIR/bench-01-raw.wasm" -- --read-only 2>&1)
echo "$TMPFS_READ_RESULTS"

rm -f "$TMPFS_DIR"/*.dat

# ==========================================
# Halycon VFS benchmark (full mode - no host cache involved)
# ==========================================
echo ""
echo "=== Running: Halycon VFS ==="
echo ""

VFS_RESULTS=$(wasmtime run "$WASM_DIR/bench-01-composed.wasm" 2>&1)
echo "$VFS_RESULTS"

# ==========================================
# Results Summary
# ==========================================
echo ""
echo "=========================================="
echo "  Results Summary"
echo "=========================================="
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""
echo "--- ext4 baseline ---"
echo "$EXT4_WRITE_RESULTS" | grep "^\[RESULT\]" | grep "seq_write" | sed 's/\[RESULT\] //'
echo "$EXT4_READ_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- tmpfs baseline ---"
echo "$TMPFS_WRITE_RESULTS" | grep "^\[RESULT\]" | grep "seq_write" | sed 's/\[RESULT\] //'
echo "$TMPFS_READ_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- Halycon VFS ---"
echo "$VFS_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'

echo ""
echo "=== Benchmark 01 Complete ==="
