# Benchmark 07: s3fs-fuse vs vfs-host S3 Sync

Compares file I/O performance between:

- **s3fs-fuse**: FUSE-based S3 mount (synchronous S3 operations)
- **vfs-host**: In-memory VFS with background S3 sync (deferred persistence)

## Key Difference

| Approach | Write Behavior | Read Behavior |
|----------|---------------|---------------|
| s3fs-fuse | Each write → S3 PUT (synchronous) | Each read → S3 GET (synchronous) |
| vfs-host | Write to memory → Background sync | Read from memory (instant) |

## Prerequisites

- LocalStack running on `localhost:4566`
- Rust toolchain with `wasm32-wasip2` target
- For s3fs comparison: Linux with s3fs-fuse installed

## Usage

```bash
# Start LocalStack
docker run -d -p 4566:4566 localstack/localstack

# Run vfs-host benchmark only (works on macOS)
./run-bench.sh

# Run both s3fs and vfs-host (Linux only)
./run-bench.sh --all
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `VFS_S3_BUCKET` | S3 bucket name | halycon-bench-07 |
| `VFS_S3_PREFIX` | S3 key prefix | vfs/ |
| `VFS_SYNC_MODE` | batch or realtime | batch |

## Expected Results

vfs-host should show significantly faster I/O throughput because:

1. **Writes** go to in-memory VFS (microseconds)
2. **Reads** are served from memory (no network latency)
3. **S3 sync** happens in background thread (doesn't block I/O)

s3fs-fuse has higher latency because every I/O operation involves:

1. FUSE kernel round-trip
2. S3 API request/response over network
