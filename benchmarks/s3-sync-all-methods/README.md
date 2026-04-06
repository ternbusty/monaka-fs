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
| seq_write | 1KB | 185.7 | 161.4 | 233.3 | 235.5 |
| | 10KB | 157.2 | 175.2 | 241.8 | 246.2 |
| | 100KB | 282.4 | 158.4 | 247.8 | 241.4 |
| | 1MB | 199.4 | 181.2 | 325.8 | 364.9 |
| seq_read | 1KB | 133.8 | 89.3 | 136.3 | 128.2 |
| | 10KB | 69.7 | 89.7 | 134.9 | 181.5 |
| | 100KB | 72.8 | 97.9 | 164.3 | 188.9 |
| | 1MB | 95.9 | 104.5 | 201.7 | 238.3 |
