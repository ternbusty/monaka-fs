#!/bin/bash
# wasi-virt-vs-vfs Benchmark Build Script
# Usage: ./build.sh
#
# Builds both wasi-virt and Halycon VFS versions of the benchmark WASM.
# Prerequisites: wasi-virt, halycon-pack, wac

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"
TESTDATA_DIR="$SCRIPT_DIR/testdata"

# wasi-virt requires a specific Rust nightly that produces WASI @0.2.3
WASI_VIRT_TOOLCHAIN="nightly-2025-06-25"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}=== Building wasi-virt-vs-vfs Benchmark ===${NC}"

# --- Generate test data ---
echo -e "${YELLOW}Generating test data...${NC}"
mkdir -p "$TESTDATA_DIR/data"
for size in 1 10 100; do
    FILE="$TESTDATA_DIR/data/${size}mb.dat"
    if [ ! -f "$FILE" ]; then
        echo "  Generating ${size}MB file..."
        dd if=/dev/urandom of="$FILE" bs=1M count=$size 2>/dev/null
    fi
done

# --- Build VFS adapter ---
echo -e "${YELLOW}Building vfs-adapter...${NC}"
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter 2>&1 | grep -v "^warning:"
VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"

# --- Build halycon-pack ---
echo -e "${YELLOW}Building halycon-pack...${NC}"
cargo build --release -p halycon-pack 2>&1 | grep -v "^warning:"
HALYCON_PACK="$ROOT_DIR/target/release/halycon-pack"

# --- wasi-virt version ---
echo -e "${YELLOW}Building wasi-virt version...${NC}"

if ! command -v wasi-virt &> /dev/null; then
    echo -e "${RED}[ERROR] wasi-virt not found.${NC}"
    echo "        Install with: cargo install --git https://github.com/bytecodealliance/wasi-virt"
    exit 1
fi

if ! rustup run "$WASI_VIRT_TOOLCHAIN" rustc --version &> /dev/null; then
    echo -e "${RED}[ERROR] Rust $WASI_VIRT_TOOLCHAIN not installed.${NC}"
    echo "        Install with: rustup install $WASI_VIRT_TOOLCHAIN"
    echo "                      rustup target add wasm32-wasip2 --toolchain $WASI_VIRT_TOOLCHAIN"
    exit 1
fi

cd "$SCRIPT_DIR/bench-app"
cargo "+$WASI_VIRT_TOOLCHAIN" build --release --target wasm32-wasip2 2>&1 | grep -v "^warning:"
BENCH_WASM_VIRT="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-wasi-virt-vs-vfs.wasm"

echo "  Creating virtualization adapter..."
wasi-virt --mount "/data=$TESTDATA_DIR/data" --allow-stdio --allow-clocks -o "$SCRIPT_DIR/virt-adapter.wasm"

echo "  Composing wasi-virt benchmark..."
wac plug --plug "$SCRIPT_DIR/virt-adapter.wasm" "$BENCH_WASM_VIRT" -o "$SCRIPT_DIR/bench-wasi-virt.wasm"
rm -f "$SCRIPT_DIR/virt-adapter.wasm"

# --- Halycon VFS version ---
echo -e "${YELLOW}Building Halycon VFS version...${NC}"

cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2 2>&1 | grep -v "^warning:"
BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-wasi-virt-vs-vfs.wasm"

echo "  Composing with vfs-adapter..."
wac plug --plug "$VFS_ADAPTER" "$BENCH_WASM" -o "$SCRIPT_DIR/bench-composed.wasm"

echo "  Packing with halycon-pack..."
"$HALYCON_PACK" embed --mount "/data=$TESTDATA_DIR/data" -o "$SCRIPT_DIR/bench-vfs.wasm" "$SCRIPT_DIR/bench-composed.wasm"
rm -f "$SCRIPT_DIR/bench-composed.wasm"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "  bench-wasi-virt.wasm  (wasi-virt embedded files)"
echo "  bench-vfs.wasm        (Halycon VFS snapshot)"
