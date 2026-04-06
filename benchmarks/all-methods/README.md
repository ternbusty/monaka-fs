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
| Sequential Read | 1MB | 0.38ms (2,608 MB/s) | 0.08ms (13,311 MB/s) | 2.99ms (334 MB/s) |
| | 10MB | 4.77ms (2,098 MB/s) | 0.80ms (12,497 MB/s) | 21.0ms (476 MB/s) |
| | 100MB | 50.7ms (1,974 MB/s) | 9.01ms (11,100 MB/s) | 249.9ms (400 MB/s) |
| Random Read | 1MB | 2.33ms (1,674 MB/s) | 0.97ms (4,030 MB/s) | 98.8ms (40 MB/s) |
| | 10MB | 2.46ms (1,588 MB/s) | 1.34ms (2,911 MB/s) | 126.3ms (31 MB/s) |
| | 100MB | 3.50ms (1,116 MB/s) | 2.74ms (1,424 MB/s) | 129.2ms (30 MB/s) |
| Sequential Write | 1MB | 0.62ms (1,620 MB/s) | 0.29ms (3,412 MB/s) | 1.72ms (582 MB/s) |
| | 10MB | 6.21ms (1,611 MB/s) | 2.65ms (3,767 MB/s) | 12.3ms (813 MB/s) |
| | 100MB | 61.6ms (1,624 MB/s) | 26.3ms (3,799 MB/s) | 132.2ms (757 MB/s) |
