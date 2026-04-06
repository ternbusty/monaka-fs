#!/bin/bash
# tmpfs-vs-vfs Benchmark Runner
# Usage: ./run.sh
#
# Runs ext4 vs tmpfs vs Monaka VFS benchmark on a Linux host.
# This script must be run on Linux (tmpfs + page cache clearing require it).

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WASM_DIR="$SCRIPT_DIR/wasm"

export PATH="$HOME/.wasmtime/bin:$HOME/.cargo/bin:$PATH"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# This benchmark requires Linux with tmpfs support
if [[ "$(uname)" != "Linux" ]]; then
    echo -e "${RED}Error: This benchmark requires Linux with tmpfs support.${NC}"
    echo "Use transfer.sh to deploy to a Linux VM, then run there."
    exit 1
fi

# Verify wasmtime is available
if ! command -v wasmtime &> /dev/null; then
    echo -e "${RED}[ERROR] wasmtime not found. Install it first.${NC}"
    exit 1
fi

# Verify WASM files exist
for f in "$WASM_DIR/bench-raw.wasm" "$WASM_DIR/bench-composed.wasm"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}[ERROR] $(basename "$f") not found in $WASM_DIR${NC}"
        exit 1
    fi
done

# Helper: create test files (5 files per size for cold read iterations)
create_test_files() {
    local dir="$1"
    echo "Creating test files in $dir..."
    for i in 0 1 2 3 4; do
        dd if=/dev/urandom of="$dir/1mb_${i}.dat" bs=1M count=1 2>/dev/null
        dd if=/dev/urandom of="$dir/10mb_${i}.dat" bs=1M count=10 2>/dev/null
        dd if=/dev/urandom of="$dir/100mb_${i}.dat" bs=1M count=100 2>/dev/null
    done
    sync
}

# Run host read benchmarks with cache clear between seq_read and random_read
run_host_read_bench() {
    local dir="$1"
    local label="$2"

    clear_cache

    echo "--- Sequential read (cache cleared) ---"
    local seq_results
    seq_results=$(wasmtime run --dir="$dir::/mnt" "$WASM_DIR/bench-raw.wasm" -- --seq-read-only 2>&1)
    echo "$seq_results" | grep "^\[RESULT\]"

    clear_cache

    echo "--- Random read (cache cleared) ---"
    local rand_results
    rand_results=$(wasmtime run --dir="$dir::/mnt" "$WASM_DIR/bench-raw.wasm" -- --random-read-only 2>&1)
    echo "$rand_results" | grep "^\[RESULT\]"

    # Store for summary
    eval "${label}_SEQ_RESULTS=\"\$seq_results\""
    eval "${label}_RAND_RESULTS=\"\$rand_results\""
}

clear_cache() {
    echo "Clearing page cache..."
    sync
    sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches' 2>/dev/null || echo -e "${YELLOW}[WARN] Could not clear cache (need sudo)${NC}"
}

echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  Benchmark: ext4 vs tmpfs vs Monaka VFS${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""

# Setup directories
EXT4_DIR=$(mktemp -d)
TMPFS_DIR="/dev/shm/monaka-bench"
mkdir -p "$TMPFS_DIR"
trap "rm -rf $TMPFS_DIR $EXT4_DIR" EXIT

# ==========================================
# ext4 benchmark
# ==========================================
echo -e "${YELLOW}=== Running: Host ext4 baseline ===${NC}"
echo ""

create_test_files "$EXT4_DIR"
clear_cache

echo "--- Write benchmark ---"
EXT4_WRITE_RESULTS=$(wasmtime run --dir="$EXT4_DIR::/mnt" "$WASM_DIR/bench-raw.wasm" 2>&1)
echo "$EXT4_WRITE_RESULTS" | grep -E "seq_write"

run_host_read_bench "$EXT4_DIR" "EXT4"

rm -f "$EXT4_DIR"/*.dat

# ==========================================
# tmpfs benchmark
# ==========================================
echo ""
echo -e "${YELLOW}=== Running: Host tmpfs baseline ===${NC}"
echo ""

create_test_files "$TMPFS_DIR"
clear_cache

echo "--- Write benchmark ---"
TMPFS_WRITE_RESULTS=$(wasmtime run --dir="$TMPFS_DIR::/mnt" "$WASM_DIR/bench-raw.wasm" 2>&1)
echo "$TMPFS_WRITE_RESULTS" | grep -E "seq_write"

run_host_read_bench "$TMPFS_DIR" "TMPFS"

rm -f "$TMPFS_DIR"/*.dat

# ==========================================
# Monaka VFS benchmark (full mode)
# ==========================================
echo ""
echo -e "${YELLOW}=== Running: Monaka VFS ===${NC}"
echo ""

VFS_RESULTS=$(wasmtime run "$WASM_DIR/bench-composed.wasm" 2>&1)
echo "$VFS_RESULTS"

# ==========================================
# Results Summary
# ==========================================
echo ""
echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  Results Summary${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""
echo "--- ext4 baseline ---"
echo "$EXT4_WRITE_RESULTS" | grep "^\[RESULT\]" | grep "seq_write" | sed 's/\[RESULT\] //'
echo "$EXT4_SEQ_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo "$EXT4_RAND_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- tmpfs baseline ---"
echo "$TMPFS_WRITE_RESULTS" | grep "^\[RESULT\]" | grep "seq_write" | sed 's/\[RESULT\] //'
echo "$TMPFS_SEQ_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo "$TMPFS_RAND_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- Monaka VFS ---"
echo "$VFS_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'

echo ""
echo -e "${GREEN}=== Benchmark Complete ===${NC}"
