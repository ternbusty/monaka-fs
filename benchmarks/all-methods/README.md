# All VFS Methods Benchmark

Compares the performance of three VFS implementation methods.

| Method | Description | Use Case |
|--------|-------------|----------|
| **Static Compose** | `wac plug` with `vfs-adapter` | Single WASM file, no runtime dependency |
| **Host Trait** | `wasmtime` with `vfs-host` | Native runtime, multiple apps share VFS |
| **RPC** | `rpc-adapter` + `vfs-rpc-server` | Network-based, cross-machine sharing |

## How to Run

```bash
cd benchmarks/all-methods

./build.sh    # Build all components
./run.sh      # Run benchmark
```

## Architecture

```
Static Compose:
  [bench-app.wasm] --wac plug--> [vfs_adapter.wasm] --> wasmtime

Host Trait:
  [bench-app.wasm] --> wasmtime + vfs-host (native Host trait impl)

RPC:
  [bench-app.wasm] --wac plug--> [rpc_adapter.wasm] --> TCP:9000 --> [vfs-rpc-server]
```

## Test Scenarios

| Operation | Description |
|-----------|-------------|
| seq_write | Sequential write of a single file |
| seq_read | Sequential read of a single file |
| random_read | 1000 random 4KB block reads at random offsets |

Each operation runs with the following parameter combinations.

| Parameter | Values |
|-----------|--------|
| File size | 1MB, 10MB, 100MB |
| Iterations | 5 (median reported) |

## Output Format

Results are printed as:

```
[RESULT] operation,size,time_ms,throughput_mb_s
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

| Operation | Size | Static Compose | Host Trait | RPC |
|-----------|------|----------------|------------|-----|
| Sequential Read | 1MB | 0.40ms (2,520 MB/s) | 0.10ms (10,381 MB/s) | 1.53ms (652 MB/s) |
| | 10MB | 5.00ms (2,001 MB/s) | 0.84ms (11,862 MB/s) | 8.66ms (1,155 MB/s) |
| | 100MB | 52.8ms (1,893 MB/s) | 10.3ms (9,730 MB/s) | 114.2ms (875 MB/s) |
| Random Read | 1MB | 2.25ms (1,736 MB/s) | 0.95ms (4,099 MB/s) | 95.8ms (41 MB/s) |
| | 10MB | 2.96ms (1,321 MB/s) | 1.36ms (2,878 MB/s) | 135.1ms (29 MB/s) |
| | 100MB | 3.35ms (1,165 MB/s) | 2.35ms (1,665 MB/s) | 133.4ms (29 MB/s) |
| Sequential Write | 1MB | 0.60ms (1,662 MB/s) | 0.25ms (4,009 MB/s) | 1.70ms (589 MB/s) |
| | 10MB | 6.16ms (1,624 MB/s) | 2.67ms (3,743 MB/s) | 12.3ms (814 MB/s) |
| | 100MB | 62.8ms (1,593 MB/s) | 25.6ms (3,903 MB/s) | 133.3ms (750 MB/s) |
