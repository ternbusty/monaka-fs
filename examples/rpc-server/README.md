# RPC Server Examples

TCP-based communication using `vfs-rpc-server`. WASM apps are composed with `rpc-adapter` via `wac plug`, then connect to a running server on port 9000.

```
App (std::fs) + rpc-adapter  --[wac plug]-->  Composed WASM  --[TCP:9000]-->  vfs-rpc-server
```

Multiple composed apps can share a single VFS instance through the same server.

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

## Build

```bash
# From repository root:

# 1. Build the RPC server
cargo build -p vfs-rpc-server --target wasm32-wasip2

# 2. Build the RPC adapter
cargo build -p rpc-adapter --target wasm32-wasip2

# 3. Build the demo apps (see ../apps/)
cargo build -p demo-writer --target wasm32-wasip2
cargo build -p demo-reader --target wasm32-wasip2
```

## Compose

```bash
# From repository root:
wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/demo-writer.wasm \
  -o target/wasm32-wasip2/debug/composed-demo-writer.wasm

wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/demo-reader.wasm \
  -o target/wasm32-wasip2/debug/composed-demo-reader.wasm
```

## Run

### Start the server

```bash
# From repository root:
wasmtime run -S inherit-network=y target/wasm32-wasip2/debug/vfs_rpc_server.wasm
```

### Run demo-writer (in another terminal)

```bash
wasmtime run -S inherit-network=y target/wasm32-wasip2/debug/composed-demo-writer.wasm
```

### Run demo-reader (in another terminal)

```bash
wasmtime run -S inherit-network=y target/wasm32-wasip2/debug/composed-demo-reader.wasm
```

### Stop the server

```bash
pkill -f vfs_rpc_server.wasm
```

## S3 Sync

The RPC server supports automatic S3 synchronization. Files written through the VFS are synced to S3.

### Prerequisites

Start LocalStack from the repository root:

```bash
docker compose up -d
```

### Build

```bash
# From repository root:
cargo build -p vfs-rpc-server --target wasm32-wasip2 --features s3-sync
```

### Start the server with S3

```bash
# From repository root:
wasmtime run -S inherit-network=y -S http \
  --env VFS_S3_BUCKET=test-vfs-bucket \
  --env VFS_S3_PREFIX=vfs/ \
  --env VFS_SYNC_MODE=batch \
  --env AWS_ENDPOINT_URL=http://localhost:4566 \
  --env AWS_ACCESS_KEY_ID=test \
  --env AWS_SECRET_ACCESS_KEY=test \
  --env AWS_REGION=ap-northeast-1 \
  target/wasm32-wasip2/debug/vfs_rpc_server.wasm
```

### Run demo-writer (in another terminal)

```bash
wasmtime run -S inherit-network=y target/wasm32-wasip2/debug/composed-demo-writer.wasm
```

### Verify S3 Sync

```bash
awslocal s3 ls s3://test-vfs-bucket/vfs/files/ --recursive
```

Expected:

```
<date> <time>         16 vfs/files/message.txt
```
