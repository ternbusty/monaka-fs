# Lock Strategy Benchmark

Measures the performance of the VFS locking strategy (DashMap + per-inode RwLock) under concurrent access.
Multiple WASM instances share a single VFS and perform file operations in parallel.

## How to Run

```
cd usecases/benchmarks/06-lock-strategy/bench-runner

./run.sh build-wasm   # Build the WASM benchmark app
./run.sh build        # Build the runtime
./run.sh run          # Run the benchmark
```

## Test Scenarios

| Scenario | Description |
|---------|------|
| read/same | Multiple threads read the same file concurrently |
| read/different | Each thread reads its own file |
| write/same | Multiple threads append to the same file |
| write/different | Each thread writes to its own file |
| mixed/same | 80% reads and 20% writes on the same file |

Each scenario runs with the following parameter combinations.

| Parameter | Values |
|-----------|--------|
| Threads | 1, 4, 8, 16 |
| Data size | 1KB, 64KB, 1MB |
| Operations per thread | 500 |

## Output Format

Results are printed in CSV format.

```
strategy,scenario,file_scope,threads,data_size,total_ops,duration_ms,throughput_ops_sec,errors,data_integrity
```

## Results

Environment:

| Item | Value |
|------|-------|
| CPU | Apple M1 Pro |
| Memory | 32 GB |
| OS | macOS 26.3.1 |
| Rust | 1.92.0 |
| wasmtime | 39.0.1 |

### Data Size: 1KB (throughput ops/sec)

| Scenario | 1 thread | 4 threads | 8 threads | 16 threads |
|---------|-------:|-------:|-------:|-------:|
| read/same | 293,765 | 380,032 | 630,989 | 615,977 |
| read/different | 823,780 | 623,911 | 652,795 | 615,600 |
| write/same | 188,342 | 277,245 | 319,374 | 341,448 |
| write/different | 293,736 | 575,229 | 376,531 | 458,153 |
| mixed/same | 412,853 | 460,648 | 359,793 | 452,358 |

### Data Size: 64KB (throughput ops/sec)

| Scenario | 1 thread | 4 threads | 8 threads | 16 threads |
|---------|-------:|-------:|-------:|-------:|
| read/same | 105,285 | 257,811 | 190,913 | 216,908 |
| read/different | 85,141 | 194,575 | 206,952 | 218,267 |
| write/same | 37,215 | 29,067 | 25,906 | 26,010 |
| write/different | 73,186 | 77,600 | 67,786 | 70,959 |
| mixed/same | 76,988 | 109,500 | 92,906 | 85,450 |

### Data Size: 1MB (throughput ops/sec)

| Scenario | 1 thread | 4 threads | 8 threads | 16 threads |
|---------|-------:|-------:|-------:|-------:|
| read/same | 12,610 | 32,887 | 34,540 | 36,040 |
| read/different | 12,961 | 28,788 | 32,598 | 34,816 |
| write/same | 4,051 | 2,161 | 1,481 | 1,494 |
| write/different | 4,831 | 6,360 | 6,117 | 4,548 |
| mixed/same | 9,074 | 6,385 | 5,652 | 5,809 |

### Data Integrity

The write/same scenario verifies that all appended lines are correctly recorded.
Data integrity is 100.0% across all thread counts and data sizes.
