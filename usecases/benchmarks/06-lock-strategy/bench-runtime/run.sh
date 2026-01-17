#!/bin/bash
# Lock Strategy Benchmark Runner
# Usage: ./run.sh [command]
#
# Commands:
#   build       - Build all binaries
#   fine        - Run lock-fine benchmark
#   global      - Run lock-global benchmark
#   unsafe      - Run lock-none benchmark
#   main        - Run main-branch benchmark (single-threaded)
#   all         - Run all benchmarks
#   compare     - Run fine vs main comparison (single-threaded)
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
    cargo build --release --bin bench-fine --bin bench-global --bin bench-unsafe --bin bench-main 2>&1 | grep -v "^warning:"
    echo -e "${GREEN}Build complete.${NC}"
}

build_wasm() {
    echo -e "${GREEN}Building WASM app...${NC}"
    cd ../bench-wasm-app
    cargo build --release --target wasm32-wasip2
    cd "$SCRIPT_DIR"
    echo -e "${GREEN}WASM build complete.${NC}"
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

run_main() {
    echo -e "${YELLOW}Running main-branch benchmark (single-threaded)...${NC}"
    cargo run --release --bin bench-main 2>&1 | grep -v "^warning:"
}

run_compare() {
    echo -e "${YELLOW}=== Comparing Fine vs Main (single-threaded) ===${NC}"
    echo ""
    echo "This compares:"
    echo "  - fine: Arc<RwLock> + DashMap (internal locks)"
    echo "  - main: Arc<Mutex<Fs>> (external lock)"
    echo ""
    echo "Running fine (single-threaded scenarios only)..."
    cargo run --release --bin bench-fine 2>&1 | grep -v "^warning:" | grep ",1,"
    echo ""
    echo "Running main..."
    cargo run --release --bin bench-main 2>&1 | grep -v "^warning:"
}

run_all() {
    echo "=== Running all benchmarks ==="
    echo ""
    run_fine
    echo ""
    run_global
    echo ""
    run_main
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
    echo "  fine        - Run lock-fine benchmark (DashMap + per-inode RwLock)"
    echo "  global      - Run lock-global benchmark (single RwLock)"
    echo "  main        - Run main-branch benchmark (external Mutex, single-threaded)"
    echo "  unsafe      - Run lock-none benchmark (no locking, UNSAFE)"
    echo "  all         - Run all benchmarks sequentially"
    echo "  compare     - Compare fine vs main (single-threaded overhead)"
    echo "  help        - Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 build && $0 fine"
    echo "  $0 compare                        # Compare locking overhead"
    echo "  $0 fine 2>&1 | tee results-fine.csv"
    echo ""
    echo "Expected results:"
    echo "  - lock-fine:   Internal RwLock + DashMap (thread-safe, lower overhead)"
    echo "  - lock-global: Single RwLock (thread-safe, higher contention)"
    echo "  - main-mutex:  External Mutex (single-threaded, baseline overhead)"
    echo "  - lock-none:   UnsafeCell (UNSAFE, may crash)"
}

case "${1:-help}" in
    build)
        build
        ;;
    build-wasm)
        build_wasm
        ;;
    fine)
        run_fine
        ;;
    global)
        run_global
        ;;
    main)
        run_main
        ;;
    unsafe)
        run_unsafe
        ;;
    all)
        run_all
        ;;
    compare)
        run_compare
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
