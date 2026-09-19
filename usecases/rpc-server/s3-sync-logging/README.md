# S3 Sync Logging

Multiple WASM replicas writing logs concurrently to a shared VFS via RPC, with automatic S3 synchronization. Several servers may share the same bucket; each append is serialized through a per-file S3 lease so no line is lost.

**Deployment method**: RPC Server (`vfs-rpc-server` with S3 sync)

```
replica-1 --+
replica-2 --+--> vfs-rpc-server (TCP:9000) --> S3 <-- vfs-rpc-server (TCP:9001) <--+-- replica-3
                     |                                       |                      +-- replica-4
               /logs/app.log                           /logs/app.log
```

## Using `monaka` CLI

```bash
# Build the app
cargo build -p logger --target wasm32-wasip2

# Compose with RPC adapter
make build-cli
target/release/monaka compose --rpc \
  target/wasm32-wasip2/debug/logger.wasm \
  -o /tmp/composed-logger.wasm

# Extract S3-enabled server and start it
target/release/monaka extract server --s3-sync -o /tmp/vfs-rpc-server.wasm

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

### Two servers, one bucket

Start a second server on another port and point some replicas at it. Both servers take the per-file lease before appending, so the log in S3 ends up with every replica's lines.

```bash
# Terminal 1 and 2: two servers on different ports
VFS_RPC_PORT=9000 wasmtime run -S inherit-network=y -S http --env VFS_RPC_PORT=9000 --env VFS_S3_BUCKET=test-vfs-bucket ... /tmp/vfs-rpc-server.wasm
VFS_RPC_PORT=9001 wasmtime run -S inherit-network=y -S http --env VFS_RPC_PORT=9001 --env VFS_S3_BUCKET=test-vfs-bucket ... /tmp/vfs-rpc-server.wasm

# Terminal 3: replicas split across the servers
wasmtime run -S inherit-network=y --env VFS_RPC_PORT=9000 --env REPLICA_ID=1 /tmp/composed-logger.wasm &
wasmtime run -S inherit-network=y --env VFS_RPC_PORT=9000 --env REPLICA_ID=2 /tmp/composed-logger.wasm &
wasmtime run -S inherit-network=y --env VFS_RPC_PORT=9001 --env REPLICA_ID=3 /tmp/composed-logger.wasm &
wasmtime run -S inherit-network=y --env VFS_RPC_PORT=9001 --env REPLICA_ID=4 /tmp/composed-logger.wasm &
wait
```

The semantics of the lease, and the errors an application sees when a lease is held elsewhere, are described in [docs/sync-semantics.md](../../../docs/sync-semantics.md).

### Environment variables

| Variable | Read by | Default | Description |
|----------|---------|---------|-------------|
| `REPLICA_ID` | logger | `1` | Label written into each line |
| `ENTRY_COUNT` | logger | `10` | Number of entries to append |
| `ENTRY_DELAY_MS` | logger | `1000` | Pause between entries |
| `VFS_RPC_PORT` | server and logger | `9000` | Server port |
| `VFS_S3_FILE_LOCK` | server | `enabled` | Per-file lease on open for write |
| `VFS_S3_FILE_LOCK_TIMEOUT_MS` | server and logger | `10000` | How long an open waits for a lease held elsewhere |

## Prerequisites

- Docker (for LocalStack)
- `awslocal` (`uv tool install awscli-local awscli`)

## Manual Setup (without `monaka` CLI)

```bash
cargo build -p vfs-rpc-server --target wasm32-wasip2 --features s3-sync
cargo build -p rpc-adapter --target wasm32-wasip2
cargo build -p logger --target wasm32-wasip2

wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/logger.wasm \
  -o /tmp/composed-logger.wasm
```
