#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/wasm32-wasip2/release"
TESTDATA_DIR="$SCRIPT_DIR/testdata"

# wasi-virt requires a specific Rust nightly that produces WASI @0.2.3
WASI_VIRT_TOOLCHAIN="nightly-2025-06-25"

echo "=== Generating test data ==="
mkdir -p "$TESTDATA_DIR/data"

# Generate test files
for size in 1 10 100; do
    FILE="$TESTDATA_DIR/data/${size}mb.dat"
    if [ ! -f "$FILE" ]; then
        echo "Generating ${size}MB file..."
        dd if=/dev/urandom of="$FILE" bs=1M count=$size 2>/dev/null
    else
        echo "${size}MB file already exists"
    fi
done

echo ""
echo "=== Building VFS adapter ==="
cd "$ROOT_DIR"
cargo build --release --target wasm32-wasip2 -p vfs-adapter

VFS_ADAPTER="$BUILD_DIR/vfs_adapter.wasm"

echo ""
echo "=========================================="
echo "  Running: wasi-virt baseline"
echo "=========================================="

# Check if wasi-virt is installed
if ! command -v wasi-virt &> /dev/null; then
    echo "[ERROR] wasi-virt not found."
    echo "        Install with: cargo install --git https://github.com/bytecodealliance/wasi-virt"
    WASI_VIRT_RESULTS=""
# Check if required toolchain is installed
elif ! rustup run "$WASI_VIRT_TOOLCHAIN" rustc --version &> /dev/null; then
    echo "[ERROR] Rust $WASI_VIRT_TOOLCHAIN not installed."
    echo "        Install with: rustup install $WASI_VIRT_TOOLCHAIN"
    echo "                      rustup target add wasm32-wasip2 --toolchain $WASI_VIRT_TOOLCHAIN"
    WASI_VIRT_RESULTS=""
else
    echo "Building benchmark app with $WASI_VIRT_TOOLCHAIN (for WASI @0.2.3 compatibility)..."
    cd "$SCRIPT_DIR/bench-app"
    cargo "+$WASI_VIRT_TOOLCHAIN" build --release --target wasm32-wasip2

    BENCH_WASM_VIRT="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-wasi-virt-vs-vfs.wasm"
    VIRT_ADAPTER="$SCRIPT_DIR/virt-adapter.wasm"
    WASI_VIRT_WASM="$SCRIPT_DIR/bench-wasi-virt.wasm"

    echo "Creating virtualization adapter with wasi-virt..."
    wasi-virt --mount "/data=$TESTDATA_DIR/data" --allow-stdio --allow-clocks -o "$VIRT_ADAPTER"

    echo "Composing with wac plug..."
    wac plug --plug "$VIRT_ADAPTER" "$BENCH_WASM_VIRT" -o "$WASI_VIRT_WASM"

    echo "Running wasi-virt benchmark..."
    WASI_VIRT_RESULTS=$(wasmtime run "$WASI_VIRT_WASM" 2>&1)
    echo "$WASI_VIRT_RESULTS"

    rm -f "$VIRT_ADAPTER" "$WASI_VIRT_WASM"
fi

echo ""
echo "=========================================="
echo "  Running: Halycon VFS (halycon-pack)"
echo "=========================================="

# Build benchmark app with stable toolchain for halycon-pack
echo "Building benchmark app with stable toolchain..."
cd "$SCRIPT_DIR/bench-app"
cargo build --release --target wasm32-wasip2

BENCH_WASM="$SCRIPT_DIR/bench-app/target/wasm32-wasip2/release/bench-wasi-virt-vs-vfs.wasm"

# Check if halycon-pack is installed
if ! command -v halycon-pack &> /dev/null; then
    echo "[ERROR] halycon-pack not found in PATH"
    echo "        Install with: cargo install --path $ROOT_DIR/crates/tools/halycon-pack"
    VFS_RESULTS=""
else
    PACKED_WASM="$SCRIPT_DIR/bench-packed.wasm"

    # Compose with VFS adapter
    COMPOSED_WASM="$SCRIPT_DIR/bench-composed.wasm"
    wac plug --plug "$VFS_ADAPTER" "$BENCH_WASM" -o "$COMPOSED_WASM"

    echo "Packing with halycon-pack..."
    halycon-pack embed --mount "/data=$TESTDATA_DIR/data" -o "$PACKED_WASM" "$COMPOSED_WASM"

    echo "Running halycon-pack benchmark..."
    VFS_RESULTS=$(wasmtime run "$PACKED_WASM" 2>&1)
    echo "$VFS_RESULTS"

    rm -f "$COMPOSED_WASM" "$PACKED_WASM"
fi

echo ""
echo "=========================================="
echo "  Comparison Summary"
echo "=========================================="

echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""

if [ -n "$WASI_VIRT_RESULTS" ]; then
    echo "--- wasi-virt ---"
    echo "$WASI_VIRT_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'
    echo ""
fi

echo "--- Halycon VFS ---"
echo "$VFS_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //'

echo ""
echo "=== Benchmark Complete ==="
