#!/bin/bash
# Static composition benchmark runner
# Compares Rc<RefCell> vs Arc<RwLock> fs-core implementations
#
# Usage:
#   ./run_static.sh build    # Build both versions
#   ./run_static.sh run      # Run benchmark
#   ./run_static.sh all      # Build and run

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$SCRIPT_DIR/../../../.."
WASM_DIR="$SCRIPT_DIR/wasm"
VFS_ADAPTER_DIR="$ROOT_DIR/crates/adapters/vfs-adapter"
BENCH_APP_DIR="$SCRIPT_DIR/../bench-wasm-app"

# Benchmark parameters
OPS=${OPS:-500}
DATA_SIZE=${DATA_SIZE:-1024}
THREAD_COUNT=${THREAD_COUNT:-8}

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

build() {
    echo -e "${GREEN}=== Building WASM components ===${NC}" >&2
    mkdir -p "$WASM_DIR"

    # Build bench-wasm-app
    echo -e "${YELLOW}Building bench-wasm-app...${NC}" >&2
    (cd "$BENCH_APP_DIR" && cargo build --release --target wasm32-wasip2)

    # vfs-adapter is part of workspace, so target is at root
    local WORKSPACE_TARGET="$ROOT_DIR/target"

    # Build vfs-adapter (Rc<RefCell> version)
    echo -e "${YELLOW}Building vfs-adapter (Rc<RefCell>)...${NC}" >&2
    (cd "$ROOT_DIR" && cargo build --release --target wasm32-wasip2 -p vfs-adapter)
    cp "$WORKSPACE_TARGET/wasm32-wasip2/release/vfs_adapter.wasm" "$WASM_DIR/vfs_adapter_single.wasm"

    # Build vfs-adapter (Arc<RwLock> version)
    echo -e "${YELLOW}Building vfs-adapter (Arc<RwLock>)...${NC}" >&2
    (cd "$ROOT_DIR" && cargo build --release --target wasm32-wasip2 -p vfs-adapter --features thread-safe)
    cp "$WORKSPACE_TARGET/wasm32-wasip2/release/vfs_adapter.wasm" "$WASM_DIR/vfs_adapter_threadsafe.wasm"

    # Static composition
    echo -e "${YELLOW}Composing WASM modules...${NC}" >&2
    local bench_app="$BENCH_APP_DIR/target/wasm32-wasip2/release/bench-lock-wasm-app.wasm"

    wac plug --plug "$WASM_DIR/vfs_adapter_single.wasm" "$bench_app" -o "$WASM_DIR/bench-single.wasm"
    wac plug --plug "$WASM_DIR/vfs_adapter_threadsafe.wasm" "$bench_app" -o "$WASM_DIR/bench-threadsafe.wasm"

    echo -e "${GREEN}Build complete.${NC}" >&2
    ls -lh "$WASM_DIR"/*.wasm >&2
}

run_benchmark() {
    local strategy="$1"
    local scenario="$2"
    local file_scope="$3"
    local wasm_file="$4"

    # Run wasmtime with environment variables
    local output
    output=$(wasmtime run \
        --env BENCH_SCENARIO="$scenario" \
        --env BENCH_FILE_SCOPE="$file_scope" \
        --env BENCH_OPS="$OPS" \
        --env BENCH_DATA_SIZE="$DATA_SIZE" \
        --env BENCH_THREAD_ID="0" \
        --env BENCH_THREAD_COUNT="$THREAD_COUNT" \
        "$wasm_file" 2>&1)

    # Parse the BENCH_RESULT line
    local result_line
    result_line=$(echo "$output" | grep "BENCH_RESULT:" || echo "")

    if [ -n "$result_line" ]; then
        # Extract elapsed_us from the result
        local elapsed_us
        elapsed_us=$(echo "$result_line" | sed 's/.*elapsed_us=\([0-9]*\).*/\1/')

        # Calculate throughput
        local throughput
        if [ "$elapsed_us" -gt 0 ]; then
            throughput=$(echo "scale=2; $OPS * 1000000 / $elapsed_us" | bc)
        else
            throughput="inf"
        fi

        echo "$strategy,$scenario,$file_scope,$OPS,$DATA_SIZE,$elapsed_us,$throughput"
    else
        echo "$strategy,$scenario,$file_scope,$OPS,$DATA_SIZE,ERROR,0" >&2
        echo "$output" >&2
    fi
}

run() {
    echo -e "${GREEN}=== Static Composition Benchmark ===${NC}" >&2
    echo "Comparing Rc<RefCell> vs Arc<RwLock> fs-core" >&2
    echo "OPS=$OPS, DATA_SIZE=$DATA_SIZE" >&2
    echo "" >&2

    # Check if wasm files exist
    if [ ! -f "$WASM_DIR/bench-single.wasm" ] || [ ! -f "$WASM_DIR/bench-threadsafe.wasm" ]; then
        echo "WASM files not found. Run './run_static.sh build' first." >&2
        exit 1
    fi

    # CSV header
    echo "strategy,scenario,file_scope,ops,data_size,elapsed_us,throughput_ops_sec"

    # Scenarios to test
    SCENARIOS="read:same read:different write:same write:different mixed:same"

    for strategy_wasm in "rc-refcell:bench-single.wasm" "arc-rwlock:bench-threadsafe.wasm"; do
        strategy="${strategy_wasm%%:*}"
        wasm_file="$WASM_DIR/${strategy_wasm##*:}"

        for scenario_scope in $SCENARIOS; do
            scenario="${scenario_scope%%:*}"
            file_scope="${scenario_scope##*:}"

            echo "Running $strategy / $scenario / $file_scope..." >&2

            # Setup fresh state for each scenario
            wasmtime run \
                --env BENCH_SCENARIO="setup" \
                --env BENCH_DATA_SIZE="$DATA_SIZE" \
                --env BENCH_THREAD_COUNT="$THREAD_COUNT" \
                "$wasm_file" 2>/dev/null || true

            # Run benchmark
            run_benchmark "$strategy" "$scenario" "$file_scope" "$wasm_file"
        done
    done

    echo "" >&2
    echo -e "${GREEN}=== Benchmark Complete ===${NC}" >&2
}

show_help() {
    echo "Static Composition Benchmark Runner"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  build   - Build vfs-adapter (both versions) and compose with bench app"
    echo "  run     - Run the benchmark"
    echo "  all     - Build and run"
    echo "  help    - Show this help"
    echo ""
    echo "Environment variables:"
    echo "  OPS=500        - Operations per scenario"
    echo "  DATA_SIZE=1024 - Data size in bytes"
}

case "${1:-help}" in
    build)
        build
        ;;
    run)
        run
        ;;
    all)
        build
        run
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        echo "Unknown command: $1" >&2
        show_help
        exit 1
        ;;
esac
