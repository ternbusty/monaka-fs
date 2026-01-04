# S3 Sync Logging Demo

This example demonstrates multiple WASM application replicas writing logs concurrently to a shared VFS, with automatic synchronization to S3.

## Prerequisites

- Docker (for LocalStack)
- AWS CLI
- wasmtime
- wac-cli

## Running the Demo

```bash
# From project root
make run-usecase-s3-sync-logging

# Or directly
./examples/usecase-s3-sync-logging/run-demo.sh
```

## What Happens

1. **LocalStack starts** - Provides S3-compatible storage locally
2. **VFS RPC Server starts** - With S3 sync enabled
3. **3 logger replicas run in parallel** - Each writes to `/logs/app.log`
4. **Logs sync to S3** - Automatically via the sync manager
5. **Verification** - Script shows S3 contents

## Architecture

```
replica-1 ────┐
              │
replica-2 ────┼──> vfs-rpc-server ──> LocalStack S3
              │         │
replica-3 ────┘    In-memory VFS
                        │
                   /logs/app.log
                        │
                        v
               s3://vfs-logs-demo/demo/files/logs/app.log
```

## Expected Output

The shared log file `/logs/app.log` will contain timestamped, interleaved entries from all replicas:

```
2026-01-03T12:34:56.789Z [replica-1] Entry 1: Processing request...
2026-01-03T12:34:56.790Z [replica-2] Entry 1: Processing request...
2026-01-03T12:34:56.791Z [replica-3] Entry 1: Processing request...
2026-01-03T12:34:56.792Z [replica-1] Entry 2: Processing request...
...
2026-01-03T12:34:56.850Z [replica-1] Completed all tasks
2026-01-03T12:34:56.851Z [replica-2] Completed all tasks
2026-01-03T12:34:56.852Z [replica-3] Completed all tasks
```

Timestamps are in ISO 8601 format (UTC).

## Environment Variables

The server uses these for S3 configuration:

| Variable | Value | Description |
|----------|-------|-------------|
| `VFS_S3_BUCKET` | `vfs-logs-demo` | S3 bucket name |
| `VFS_S3_PREFIX` | `demo/` | S3 key prefix |
| `AWS_ENDPOINT_URL` | `http://localhost:4566` | LocalStack endpoint |
| `AWS_REGION` | `us-east-1` | AWS region |

## How S3 Sync Works

1. Applications write to the in-memory VFS using `std::fs` APIs
2. The VFS supports append mode normally
3. Every 5 seconds (flush_interval), modified files are uploaded to S3
4. S3 doesn't support append, so the **entire file** is uploaded each time
5. On server restart, files are restored from S3
