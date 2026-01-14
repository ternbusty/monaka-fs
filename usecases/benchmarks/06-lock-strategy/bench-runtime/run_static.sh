#!/bin/bash
# Static composition benchmark runner
# Compares main (Rc<RefCell>) vs fine (Arc<RwLock> + DashMap) fs-core implementations
#
# Usage: ./run_static.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WASM_DIR="$SCRIPT_DIR/wasm"

# Benchmark parameters
OPS=${OPS:-500}
DATA_SIZE=${DATA_SIZE:-1024}
THREAD_COUNT=${THREAD_COUNT:-8}

echo "=== Static Composition Benchmark ===" >&2
echo "Comparing main (Rc<RefCell>) vs fine (Arc<RwLock> + DashMap)" >&2
echo "OPS=$OPS, DATA_SIZE=$DATA_SIZE, THREAD_COUNT=$THREAD_COUNT" >&2
echo "" >&2

# CSV header
echo "strategy,scenario,file_scope,ops,data_size,elapsed_us,throughput_ops_sec"

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

# Run setup first for each strategy
echo "Setting up main..." >&2
wasmtime run \
    --env BENCH_SCENARIO="setup" \
    --env BENCH_DATA_SIZE="$DATA_SIZE" \
    --env BENCH_THREAD_COUNT="$THREAD_COUNT" \
    "$WASM_DIR/bench-main.wasm" 2>&1 | grep -v "^$" >&2 || true

echo "Setting up fine..." >&2
wasmtime run \
    --env BENCH_SCENARIO="setup" \
    --env BENCH_DATA_SIZE="$DATA_SIZE" \
    --env BENCH_THREAD_COUNT="$THREAD_COUNT" \
    "$WASM_DIR/bench-fine.wasm" 2>&1 | grep -v "^$" >&2 || true

echo "" >&2

# Scenarios to test
SCENARIOS="read:same read:different write:same write:different mixed:same"

for strategy in main fine; do
    wasm_file="$WASM_DIR/bench-$strategy.wasm"

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
echo "=== Benchmark Complete ===" >&2
