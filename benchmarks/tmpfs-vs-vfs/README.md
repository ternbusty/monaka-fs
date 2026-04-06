# tmpfs vs VFS Benchmark

Measures the performance overhead of Monaka's VFS abstraction layer compared to host-level filesystems (ext4 and tmpfs).

## How to Run

```bash
cd benchmarks/tmpfs-vs-vfs

# Build WASM app and compose with vfs-adapter
./build.sh

# Transfer to Linux VM (ext4/tmpfs tests require Linux)
./transfer.sh [vm-name]

# Run on the VM
./run.sh
```

The VFS benchmark can also run on macOS directly via wasmtime:

```bash
wasmtime run bench-composed.wasm
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
| Host CPU | Apple M1 Pro |
| VM | multipass (aarch64, 8 cores, 8 GB RAM) |
| VM OS | Ubuntu 24.04.3 LTS |
| wasmtime | 40.0.1 |

| Operation | Size | Monaka VFS | tmpfs | ext4 |
|-----------|------|-------------|-------|------|
| Sequential Read | 1MB | 0.43ms (2,355 MB/s) | 0.17ms (6,050 MB/s) | 1.17ms (856 MB/s) |
| | 10MB | 4.39ms (2,277 MB/s) | 2.10ms (4,763 MB/s) | 7.52ms (1,330 MB/s) |
| | 100MB | 46.7ms (2,143 MB/s) | 31.6ms (3,160 MB/s) | 57.4ms (1,743 MB/s) |
| Random Read | 1MB | 2.40ms (1,625 MB/s) | 1.36ms (2,877 MB/s) | 17.1ms (229 MB/s) |
| | 10MB | 2.51ms (1,554 MB/s) | 1.67ms (2,345 MB/s) | 115.7ms (34 MB/s) |
| | 100MB | 2.63ms (1,483 MB/s) | 1.93ms (2,025 MB/s) | 145.4ms (27 MB/s) |
| Sequential Write | 1MB | 0.73ms (1,373 MB/s) | 0.25ms (3,948 MB/s) | 3.18ms (314 MB/s) |
| | 10MB | 7.41ms (1,350 MB/s) | 2.77ms (3,614 MB/s) | 22.0ms (455 MB/s) |
| | 100MB | 68.6ms (1,459 MB/s) | 25.8ms (3,883 MB/s) | 125.7ms (796 MB/s) |
