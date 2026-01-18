# Benchmark 08: S3 Sync - All Methods Comparison

This benchmark compares four S3 synchronization methods in full S3 passthrough mode:

| # | Method | Description |
|---|--------|-------------|
| 1 | **s3fs-fuse** | Direct S3 mount via FUSE |
| 2 | **vfs-host** | Host trait implementation with S3 sync |
| 3 | **wac-plug** | WASM composition with vfs-adapter |
| 4 | **RPC** | RPC-based VFS with vfs-rpc-server |

## S3 Passthrough Mode

All VFS implementations run with identical S3 passthrough settings for fair comparison:

- `VFS_SYNC_MODE=realtime` - Immediate S3 PUT after each write
- `VFS_READ_MODE=s3` - Read-through from S3 on every read
- `VFS_METADATA_MODE=s3` - HEAD request on every file open

This provides an apple-to-apple comparison with s3fs-fuse which also performs synchronous S3 operations.

## Prerequisites

- Linux VM with:
  - Docker (for LocalStack)
  - s3fs-fuse installed
  - wasmtime installed
  - AWS CLI configured

## Build and Transfer

On the development machine (macOS):

```bash
cd benchmarks/08-s3-sync-all-methods
./transfer-to-vm.sh ubuntu@your-vm-host
```

This will:
1. Build the benchmark app (WASM)
2. Build the vfs-host runtime
3. Build and compose vfs-adapter and rpc-adapter
4. Transfer all files to the VM

## Running the Benchmark

On the VM:

```bash
cd /home/ubuntu/halycon-bench/08
./run-bench-vm.sh
```

### Using Real S3/GCS

Create a `.env` file with your credentials:

```bash
AWS_ACCESS_KEY_ID=your-key
AWS_SECRET_ACCESS_KEY=your-secret
AWS_REGION=your-region
AWS_ENDPOINT_URL=https://storage.googleapis.com  # For GCS
VFS_S3_BUCKET=your-bucket
VFS_S3_PREFIX=benchmark/
```

## Expected Results

The benchmark measures sequential write and read performance for various file sizes (1KB, 10KB, 100KB, 1MB).

Example output:
```
=== Results Summary ===

--- s3fs-fuse ---
seq_write,1KB,50.123,0.02
seq_read,1KB,45.678,0.02
...

--- vfs-host ---
seq_write,1KB,25.456,0.04
seq_read,1KB,20.123,0.05
...

--- wac-plug ---
seq_write,1KB,30.789,0.03
seq_read,1KB,25.456,0.04
...

--- RPC ---
seq_write,1KB,35.123,0.03
seq_read,1KB,30.789,0.03
...

=== Total Time Comparison (E2E) ===
s3fs-fuse:  12345.678ms
vfs-host:   8901.234ms
wac-plug:   9567.890ms
RPC:        10234.567ms
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     WASM Benchmark App                      │
│                    (bench-08-app.wasm)                      │
└─────────────────────────────────────────────────────────────┘
            │              │              │              │
     ┌──────┴──────┐ ┌────┴────┐ ┌──────┴──────┐ ┌─────┴─────┐
     │  s3fs-fuse  │ │ vfs-host│ │  vfs-adapter│ │rpc-adapter│
     │   (FUSE)    │ │(wasmtime│ │ (wac plug)  │ │  (TCP)    │
     └──────┬──────┘ │  host)  │ └──────┬──────┘ └─────┬─────┘
            │        └────┬────┘        │              │
            │             │             │         ┌────┴────┐
            │             │             │         │vfs-rpc- │
            │             │             │         │ server  │
            │             │             │         └────┬────┘
            │             │             │              │
     ┌──────┴─────────────┴─────────────┴──────────────┴─────┐
     │                        S3 / GCS                        │
     └────────────────────────────────────────────────────────┘
```
