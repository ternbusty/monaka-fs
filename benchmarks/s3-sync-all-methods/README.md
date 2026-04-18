# S3 Sync All Methods Benchmark

Compares four S3 synchronization methods in full S3 passthrough mode.

| Method | Description |
|--------|-------------|
| **s3fs-fuse** | Direct S3 mount via FUSE (Linux kernel) |
| **vfs-host** | Host trait implementation with S3 sync |
| **wac-plug** | WASM composition with vfs-adapter |
| **RPC** | rpc-adapter + vfs-rpc-server |

All methods run with identical S3 passthrough settings:
- `VFS_SYNC_MODE=realtime` — immediate S3 PUT after each write
- `VFS_READ_MODE=s3` — read-through from S3 on every read
- `VFS_METADATA_MODE=s3` — HEAD request on every file open

## How to Run

```bash
cd benchmarks/s3-sync-all-methods

./build.sh              # Build all components (macOS)
./transfer.sh [vm-name] # Transfer to Linux VM
./run.sh                # Run on the VM
```

### Prerequisites

- Linux VM with s3fs-fuse, wasmtime, AWS CLI
- `.env` file with S3/GCS credentials (required)

### S3 Backend

A `.env` file with S3 or GCS (S3-compatible) credentials is required:

```bash
AWS_ACCESS_KEY_ID=your-key
AWS_SECRET_ACCESS_KEY=your-secret
AWS_REGION=your-region
AWS_ENDPOINT_URL=https://storage.googleapis.com  # for GCS
VFS_S3_BUCKET=your-bucket
VFS_S3_PREFIX=benchmark/
```

`transfer.sh` automatically transfers `.env` to the VM if present.

## Test Scenarios

| Operation | Description |
|-----------|-------------|
| seq_write | Sequential write with immediate S3 PUT |
| seq_read | Sequential read via S3 GET |

Each operation runs with the following parameter combinations.

| Parameter | Values |
|-----------|--------|
| File size | 1KB, 10KB, 100KB, 1MB |
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
| VM | multipass (aarch64, 8 cores, 8 GB RAM) |
| VM OS | Ubuntu 24.04.3 LTS |
| S3 backend | GCS (S3-compatible, ap-northeast-1) |
| wasmtime | 40.0.1 |

### Per-operation latency (ms)

| Operation | Size | s3fs-fuse | vfs-host | wac-plug | RPC |
|-----------|------|-----------|----------|----------|-----|
| seq_write | 1KB | 178.3 | 149.7 | 232.9 | 241.5 |
| | 10KB | 184.8 | 158.9 | 225.2 | 249.8 |
| | 100KB | 217.2 | 156.0 | 245.4 | 247.8 |
| | 1MB | 225.4 | 183.6 | 290.9 | 281.0 |
| seq_read | 1KB | 67.1 | 83.3 | 132.6 | 138.4 |
| | 10KB | 171.5 | 90.6 | 138.5 | 145.1 |
| | 100KB | 84.5 | 86.6 | 168.8 | 171.4 |
| | 1MB | 93.3 | 124.8 | 216.3 | 221.0 |
