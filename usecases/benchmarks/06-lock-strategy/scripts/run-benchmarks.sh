#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHMARK_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$BENCHMARK_DIR/../../../.." && pwd)"

echo "=== Lock Strategy Benchmark ==="
echo "Project root: $PROJECT_ROOT"
echo "Benchmark dir: $BENCHMARK_DIR"
echo ""

# Build WASM app
echo "Building WASM app..."
cd "$BENCHMARK_DIR/bench-wasm-app"
cargo build --target wasm32-wasip2 --release
echo "WASM app built."
echo ""

# Create results directory
RESULTS_DIR="$BENCHMARK_DIR/results"
mkdir -p "$RESULTS_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_FILE="$RESULTS_DIR/benchmark_${TIMESTAMP}.csv"

echo "Results will be saved to: $RESULTS_FILE"
echo ""

cd "$BENCHMARK_DIR/bench-runtime"

# Build and run lock-fine strategy
echo "=== Building and running lock-fine (production: DashMap + per-inode RwLock) ==="
cargo build --release --no-default-features --features lock-fine
./target/release/bench-lock-runtime | tee "$RESULTS_DIR/lock-fine_${TIMESTAMP}.txt"
echo ""

# Build and run lock-global strategy
echo "=== Building and running lock-global (HashMap + single RwLock) ==="
cargo build --release --no-default-features --features lock-global
./target/release/bench-lock-runtime | tee "$RESULTS_DIR/lock-global_${TIMESTAMP}.txt"
echo ""

# Build and run lock-none strategy
echo "=== Building and running lock-none (HashMap + no locking, UNSAFE) ==="
cargo build --release --no-default-features --features lock-none
./target/release/bench-lock-runtime | tee "$RESULTS_DIR/lock-none_${TIMESTAMP}.txt"
echo ""

# Combine results into CSV
echo "=== Combining Results ==="
{
    echo "strategy,scenario,file_scope,threads,total_ops,duration_ms,throughput_ops_sec,errors"
    grep -h "^lock-" "$RESULTS_DIR/lock-fine_${TIMESTAMP}.txt" "$RESULTS_DIR/lock-global_${TIMESTAMP}.txt" "$RESULTS_DIR/lock-none_${TIMESTAMP}.txt" 2>/dev/null || true
} > "$RESULTS_FILE"

echo "=== Benchmark Complete ==="
echo "Results saved to: $RESULTS_FILE"
echo ""
echo "Individual results:"
echo "  - $RESULTS_DIR/lock-fine_${TIMESTAMP}.txt"
echo "  - $RESULTS_DIR/lock-global_${TIMESTAMP}.txt"
echo "  - $RESULTS_DIR/lock-none_${TIMESTAMP}.txt"
