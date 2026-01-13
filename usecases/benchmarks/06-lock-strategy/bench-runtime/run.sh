#!/bin/bash
# Lock Strategy Benchmark Runner
# Usage: ./run.sh [command]
#
# Commands:
#   build       - Build all binaries
#   test        - Run quick correctness test
#   fine        - Run lock-fine benchmark
#   global      - Run lock-global benchmark
#   unsafe      - Run lock-none benchmark
#   all         - Run all benchmarks
#   help        - Show this help

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

build() {
    echo -e "${GREEN}Building all binaries...${NC}"
    cargo build --release --bin bench-fine --bin bench-global --bin bench-unsafe --bin test-correctness 2>&1 | grep -v "^warning:"
    echo -e "${GREEN}Build complete.${NC}"
}

build_wasm() {
    echo -e "${GREEN}Building WASM app...${NC}"
    cd ../bench-wasm-app
    cargo build --release --target wasm32-wasip2
    cd "$SCRIPT_DIR"
    echo -e "${GREEN}WASM build complete.${NC}"
}

test_correctness() {
    echo -e "${YELLOW}Running correctness test...${NC}"
    cargo run --release --bin test-correctness 2>&1 | grep -v "^warning:"
}

run_fine() {
    echo -e "${YELLOW}Running lock-fine benchmark...${NC}"
    cargo run --release --bin bench-fine 2>&1 | grep -v "^warning:"
}

run_global() {
    echo -e "${YELLOW}Running lock-global benchmark...${NC}"
    cargo run --release --bin bench-global 2>&1 | grep -v "^warning:"
}

run_unsafe() {
    echo -e "${YELLOW}Running lock-none benchmark...${NC}"
    echo -e "${RED}WARNING: This may crash due to data races${NC}"
    cargo run --release --bin bench-unsafe 2>&1 | grep -v "^warning:" || true
}

run_all() {
    echo "=== Running all benchmarks ==="
    echo ""
    run_fine
    echo ""
    run_global
    echo ""
    run_unsafe
}

show_help() {
    echo "Lock Strategy Benchmark Runner"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  build       - Build all binaries"
    echo "  build-wasm  - Build WASM benchmark app"
    echo "  test        - Run quick correctness test (recommended first)"
    echo "  fine        - Run lock-fine benchmark (DashMap + per-inode RwLock)"
    echo "  global      - Run lock-global benchmark (single RwLock)"
    echo "  unsafe      - Run lock-none benchmark (no locking, UNSAFE)"
    echo "  all         - Run all benchmarks sequentially"
    echo "  help        - Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 build && $0 test    # Build and run correctness test"
    echo "  $0 fine 2>&1 | tee results-fine.csv"
    echo ""
    echo "Expected results:"
    echo "  - lock-fine:   100% data integrity, medium throughput"
    echo "  - lock-global: 100% data integrity, lower throughput"
    echo "  - lock-none:   <100% integrity or crash, highest throughput"
}

case "${1:-help}" in
    build)
        build
        ;;
    build-wasm)
        build_wasm
        ;;
    test)
        test_correctness
        ;;
    fine)
        run_fine
        ;;
    global)
        run_global
        ;;
    unsafe)
        run_unsafe
        ;;
    all)
        run_all
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        echo ""
        show_help
        exit 1
        ;;
esac
