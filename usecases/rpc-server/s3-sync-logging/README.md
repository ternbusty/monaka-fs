# S3 Sync Logging

Multiple WASM replicas writing logs concurrently to a shared VFS via RPC, with automatic S3 synchronization.

**Deployment method**: [RPC Server](../../examples/rpc-server/) (`vfs-rpc-server` with S3 sync)

```
replica-1 --+
replica-2 --+--> vfs-rpc-server (TCP:9000) --> S3
replica-3 --+        |
              /logs/app.log
```

## Prerequisites

- Docker (for LocalStack)
- `awslocal` (`uv tool install awscli-local awscli`)

## Build

```bash
# From repository root:
cargo build -p vfs-rpc-server --target wasm32-wasip2 --features s3-sync
cargo build -p rpc-adapter --target wasm32-wasip2
cargo build -p logger --target wasm32-wasip2

# Compose
wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/logger.wasm \
  -o target/wasm32-wasip2/debug/composed-logger.wasm
```

## Run

Start LocalStack:

```bash
# From repository root:
docker compose up -d
```

Start the server (terminal 1):

```bash
# From repository root:
wasmtime run -S inherit-network=y -S http \
  --env VFS_S3_BUCKET=test-vfs-bucket \
  --env AWS_ENDPOINT_URL=http://localhost:4566 \
  --env AWS_ACCESS_KEY_ID=test \
  --env AWS_SECRET_ACCESS_KEY=test \
  --env AWS_REGION=ap-northeast-1 \
  target/wasm32-wasip2/debug/vfs_rpc_server.wasm
```

Run replicas (terminal 2):

```bash
wasmtime run -S inherit-network=y --env REPLICA_ID=1 target/wasm32-wasip2/debug/composed-logger.wasm &
wasmtime run -S inherit-network=y --env REPLICA_ID=2 target/wasm32-wasip2/debug/composed-logger.wasm &
wasmtime run -S inherit-network=y --env REPLICA_ID=3 target/wasm32-wasip2/debug/composed-logger.wasm &
wait
```

## Verify

```bash
awslocal s3 cp s3://test-vfs-bucket/vfs/files/logs/app.log -
```
