#!/bin/bash
# Lock Strategy Benchmark Runner
# Usage: ./run.sh [command]
#
# Commands:
#   build       - Build benchmark binary
#   build-wasm  - Build WASM benchmark app
#   run         - Run benchmark
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
    echo -e "${GREEN}Building benchmark binary...${NC}"
    cargo build --release --bin bench-fine 2>&1 | grep -v "^warning:"
    echo -e "${GREEN}Build complete.${NC}"
}

build_wasm() {
    echo -e "${GREEN}Building WASM app...${NC}"
    cd ../bench-wasm-app
    cargo build --release --target wasm32-wasip2
    cd "$SCRIPT_DIR"
    echo -e "${GREEN}WASM build complete.${NC}"
}

run_bench() {
    echo -e "${YELLOW}Running benchmark...${NC}"
    cargo run --release --bin bench-fine 2>&1 | grep -v "^warning:"
}

show_help() {
    echo "Lock Strategy Benchmark Runner"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  build       - Build benchmark binary"
    echo "  build-wasm  - Build WASM benchmark app"
    echo "  run         - Run benchmark"
    echo "  help        - Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 build && $0 run"
    echo "  $0 run 2>&1 | tee results.csv"
}

case "${1:-help}" in
    build)
        build
        ;;
    build-wasm)
        build_wasm
        ;;
    run)
        run_bench
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
