# wasi-virt vs VFS Benchmark

Compares read performance between two approaches for embedding files into WASM binaries:
- **wasi-virt** (BytecodeAlliance): Static file virtualization
- **Halycon VFS**: Snapshot embedding via `halycon-pack`

## How to Run

```bash
cd benchmarks/wasi-virt-vs-vfs

./build.sh    # Build both WASM variants (requires wasi-virt, halycon-pack)
./run.sh      # Run benchmark
```

### Prerequisites

- `wasi-virt`: `cargo install --git https://github.com/bytecodealliance/wasi-virt`
- Rust nightly `nightly-2025-06-25` with `wasm32-wasip2` target (required by wasi-virt)
- `halycon-pack`: `cargo install --path crates/tools/halycon-pack`

## Test Scenarios

| Operation | Description |
|-----------|-------------|
| seq_read | Sequential read of entire file |
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
| Rust | 1.92.0 (stable) / nightly-2025-06-25 (wasi-virt) |
| wasmtime | 39.0.1 |

| Operation | Size | Halycon VFS | wasi-virt |
|-----------|------|-------------|-----------|
| Sequential Read | 1MB | 0.38ms (2,655 MB/s) | 0.45ms (2,221 MB/s) |
| | 10MB | 4.27ms (2,340 MB/s) | 4.32ms (2,314 MB/s) |
| | 100MB | 42.9ms (2,332 MB/s) | 54.4ms (1,839 MB/s) |
| Random Read | 1MB | 2.10ms (1,856 MB/s) | 2.72ms (1,435 MB/s) |
| | 10MB | 2.41ms (1,623 MB/s) | 2.37ms (1,645 MB/s) |
| | 100MB | 2.37ms (1,645 MB/s) | 2.52ms (1,550 MB/s) |
