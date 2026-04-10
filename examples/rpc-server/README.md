# RPC Server Examples

TCP-based communication using `vfs-rpc-server`. WASM apps are composed with `rpc-adapter`, then connect to a running server on port 9000.

```
App (std::fs) + rpc-adapter  -->  Composed WASM  --[TCP:9000]-->  vfs-rpc-server
```

Multiple composed apps can share a single VFS instance through the same server.

## Using `monaka` CLI

```bash
# Build demo apps
cargo build -p demo-writer -p demo-reader --target wasm32-wasip2

# Compose with RPC adapter
make build-cli
target/release/monaka compose --rpc target/wasm32-wasip2/debug/demo-writer.wasm -o /tmp/rpc-writer.wasm
target/release/monaka compose --rpc target/wasm32-wasip2/debug/demo-reader.wasm -o /tmp/rpc-reader.wasm

# Extract and start the RPC server
target/release/monaka extract server -o /tmp/vfs-rpc-server.wasm
wasmtime run -S inherit-network=y /tmp/vfs-rpc-server.wasm

# In another terminal: run writer then reader
wasmtime run -S inherit-network=y /tmp/rpc-writer.wasm /message.txt "Hello from Writer"
wasmtime run -S inherit-network=y /tmp/rpc-reader.wasm /message.txt
```

### With S3 Sync

```bash
# Extract S3-enabled server
target/release/monaka extract server --s3-sync -o /tmp/vfs-rpc-server-s3.wasm

# Start LocalStack
docker compose up -d

# Start server with S3
wasmtime run -S inherit-network=y -S http \
  --env VFS_S3_BUCKET=test-vfs-bucket \
  --env VFS_S3_PREFIX=vfs/ \
  --env VFS_SYNC_MODE=batch \
  --env AWS_ENDPOINT_URL=http://localhost:4566 \
  --env AWS_ACCESS_KEY_ID=test \
  --env AWS_SECRET_ACCESS_KEY=test \
  --env AWS_REGION=ap-northeast-1 \
  /tmp/vfs-rpc-server-s3.wasm
```

# Verify S3 sync

```
wasmtime run -S inherit-network=y /tmp/rpc-writer.wasm /message.txt 'Hello'
awslocal s3 ls s3://test-vfs-bucket/ --recursive
```

## Manual Setup (without `monaka` CLI)

### Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

### Build

```bash
# From repository root:
cargo build -p vfs-rpc-server --target wasm32-wasip2
cargo build -p rpc-adapter --target wasm32-wasip2
cargo build -p demo-writer -p demo-reader --target wasm32-wasip2
```

### Compose

```bash
wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/demo-writer.wasm \
  -o /tmp/rpc-writer.wasm

wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/demo-reader.wasm \
  -o /tmp/rpc-reader.wasm
```

### Run

```bash
# Start server
wasmtime run -S inherit-network=y target/wasm32-wasip2/debug/vfs_rpc_server.wasm

# In other terminals:
wasmtime run -S inherit-network=y /tmp/rpc-writer.wasm /message.txt 'Hello'
wasmtime run -S inherit-network=y /tmp/rpc-reader.wasm /message.txt
```

## Running demo-fs-operations (all FS operations including rename)

```bash
# Build
cargo build -p demo-fs-operations -p vfs-rpc-server -p rpc-adapter --target wasm32-wasip2

# Compose with RPC adapter
wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/demo-fs-operations.wasm \
  -o /tmp/rpc-demo-fs-operations.wasm

# Start server (terminal 1)
wasmtime run -S inherit-network=y target/wasm32-wasip2/debug/vfs_rpc_server.wasm

# Run demo (terminal 2)
wasmtime run -S inherit-network=y /tmp/rpc-demo-fs-operations.wasm
```
