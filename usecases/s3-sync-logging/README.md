# S3 Sync Logging Demo

This example demonstrates multiple WASM application replicas writing logs concurrently to a shared VFS, with automatic synchronization to S3.

## Prerequisites

- Docker (for LocalStack)
- AWS CLI
- wasmtime
- wac-cli

## Running the Demo

### Build

```bash
cargo build --target wasm32-wasip2 -p vfs-rpc-server -p rpc-adapter -p logger
```

wac compose

```bash
wac plug \
    --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
    target/wasm32-wasip2/debug/logger.wasm \
    -o target/wasm32-wasip2/debug/composed-logger.wasm
```

### Prepare

start localstack

```
docker compose up
```

create bucket

```
awslocal s3 mb s3://vfs-logs-demo
```

### Execute

start server

```bash
  wasmtime run -S inherit-network=y -S http \
    --env VFS_S3_BUCKET=vfs-logs-demo \
    --env AWS_ENDPOINT_URL=http://localhost:4566 \
    --env AWS_ACCESS_KEY_ID=test \
    --env AWS_SECRET_ACCESS_KEY=test \
    --env AWS_REGION=us-east-1 \
    target/wasm32-wasip2/debug/vfs_rpc_server.wasm
```

start apps in another terminal

```bash
wasmtime run -S inherit-network=y --env REPLICA_ID=1 target/wasm32-wasip2/debug/composed-logger.wasm &
wasmtime run -S inherit-network=y --env REPLICA_ID=2 target/wasm32-wasip2/debug/composed-logger.wasm &
wasmtime run -S inherit-network=y --env REPLICA_ID=3 target/wasm32-wasip2/debug/composed-logger.wasm &
```

### Confirm

```
awslocal s3 cp s3://vfs-logs-demo/vfs/files/logs/app.log -
```

expected output

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
