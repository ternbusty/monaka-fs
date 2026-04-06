# S3 Sync Logging

Multiple WASM replicas writing logs concurrently to a shared VFS via RPC, with automatic S3 synchronization.

**Deployment method**: RPC Server (`vfs-rpc-server` with S3 sync)

```
replica-1 --+
replica-2 --+--> vfs-rpc-server (TCP:9000) --> S3
replica-3 --+        |
              /logs/app.log
```

## Using `halycon` CLI

```bash
# Build the app
cargo build -p logger --target wasm32-wasip2

# Compose with RPC adapter
halycon compose --rpc \
  target/wasm32-wasip2/debug/logger.wasm \
  -o /tmp/composed-logger.wasm

# Extract S3-enabled server and start it
halycon extract server --s3-sync -o /tmp/vfs-rpc-server.wasm

# Start LocalStack
docker compose up -d

# Start server (terminal 1)
wasmtime run -S inherit-network=y -S http \
  --env VFS_S3_BUCKET=test-vfs-bucket \
  --env AWS_ENDPOINT_URL=http://localhost:4566 \
  --env AWS_ACCESS_KEY_ID=test \
  --env AWS_SECRET_ACCESS_KEY=test \
  --env AWS_REGION=ap-northeast-1 \
  /tmp/vfs-rpc-server.wasm

# Run replicas (terminal 2)
wasmtime run -S inherit-network=y --env REPLICA_ID=1 /tmp/composed-logger.wasm &
wasmtime run -S inherit-network=y --env REPLICA_ID=2 /tmp/composed-logger.wasm &
wasmtime run -S inherit-network=y --env REPLICA_ID=3 /tmp/composed-logger.wasm &
wait
```

### Verify

```bash
awslocal s3 cp s3://test-vfs-bucket/vfs/files/logs/app.log -
```

## Prerequisites

- Docker (for LocalStack)
- `awslocal` (`uv tool install awscli-local awscli`)

## Manual Setup (without `halycon` CLI)

```bash
cargo build -p vfs-rpc-server --target wasm32-wasip2 --features s3-sync
cargo build -p rpc-adapter --target wasm32-wasip2
cargo build -p logger --target wasm32-wasip2

wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/logger.wasm \
  -o /tmp/composed-logger.wasm
```
