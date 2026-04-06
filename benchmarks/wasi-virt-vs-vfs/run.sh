#!/bin/bash
# wasi-virt-vs-vfs Benchmark Runner
# Usage: ./run.sh
#
# Runs wasi-virt vs Halycon VFS read performance comparison.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Verify wasmtime
if ! command -v wasmtime &> /dev/null; then
    echo -e "${RED}[ERROR] wasmtime not found.${NC}"
    exit 1
fi

# Verify built files
for f in "$SCRIPT_DIR/bench-wasi-virt.wasm" "$SCRIPT_DIR/bench-vfs.wasm"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}[ERROR] $(basename "$f") not found. Run ./build.sh first.${NC}"
        exit 1
    fi
done

echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  Benchmark: wasi-virt vs Halycon VFS${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""

# --- wasi-virt ---
echo -e "${YELLOW}=== Running: wasi-virt ===${NC}"
WASI_VIRT_RESULTS=$(wasmtime run "$SCRIPT_DIR/bench-wasi-virt.wasm" 2>&1)
echo "$WASI_VIRT_RESULTS"

echo ""

# --- Halycon VFS ---
echo -e "${YELLOW}=== Running: Halycon VFS ===${NC}"
VFS_RESULTS=$(wasmtime run "$SCRIPT_DIR/bench-vfs.wasm" 2>&1)
echo "$VFS_RESULTS"

# --- Summary ---
echo ""
echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  Results Summary${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""
echo "--- wasi-virt ---"
echo "$WASI_VIRT_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
echo ""
echo "--- Halycon VFS ---"
echo "$VFS_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'

echo ""
echo -e "${GREEN}=== Benchmark Complete ===${NC}"
