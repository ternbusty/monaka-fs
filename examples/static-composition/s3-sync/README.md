# Static Composition + S3 Sync Demo

VFS with S3 synchronization through static composition. The VFS + S3 sync is embedded directly into the WASM component.

## Using `halycon` CLI

```bash
# Build the demo app (standalone package)
cd examples/static-composition/s3-sync && cargo build --target wasm32-wasip2 && cd ../../..

# Compose with S3 sync adapter
make build-cli
target/release/halycon compose --s3-sync \
  examples/static-composition/s3-sync/target/wasm32-wasip2/debug/static-s3-demo.wasm \
  -o /tmp/static-s3-composed.wasm

# Start LocalStack (from repository root)
docker compose up -d

# Run
wasmtime run -S inherit-network=y -S http \
    --env VFS_S3_BUCKET=test-vfs-bucket \
    --env VFS_S3_PREFIX=demo/ \
    --env VFS_SYNC_MODE=realtime \
    --env AWS_ENDPOINT_URL=http://localhost:4566 \
    --env AWS_ACCESS_KEY_ID=test \
    --env AWS_SECRET_ACCESS_KEY=test \
    --env AWS_REGION=ap-northeast-1 \
    /tmp/static-s3-composed.wasm
```

## Verify S3 Sync

```bash
awslocal s3 ls s3://test-vfs-bucket/demo/ --recursive
awslocal s3 cp s3://test-vfs-bucket/demo/files/data/config.json -
```

## Architecture

```
┌─────────────────────────────────────────────┐
│           Composed WASM Component           │
│  ┌─────────────┐    ┌───────────────────┐   │
│  │  App (demo) │───>│    vfs-adapter    │   │
│  └─────────────┘    │   + s3-sync       │   │
│                     │  ┌─────────────┐  │   │
│                     │  │ In-memory   │  │   │
│                     │  │    VFS      │  │   │
│                     │  └──────┬──────┘  │   │
│                     └─────────┼─────────┘   │
└───────────────────────────────┼─────────────┘
                                │ WASI HTTP
                                v
                        ┌───────────────┐
                        │  LocalStack   │
                        │     S3        │
                        └───────────────┘
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `VFS_S3_BUCKET` | S3 bucket name (required) | - |
| `VFS_S3_PREFIX` | Key prefix for synced files | `vfs/` |
| `VFS_SYNC_MODE` | `batch` or `realtime` | `batch` |
| `VFS_FLUSH_INTERVAL_SECS` | Batch flush interval (seconds) | `5` |
| `AWS_ENDPOINT_URL` | S3 endpoint (for LocalStack) | - |
| `AWS_ACCESS_KEY_ID` | AWS credential | - |
| `AWS_SECRET_ACCESS_KEY` | AWS credential | - |
| `AWS_REGION` | AWS region | - |

## Manual Setup (without `halycon` CLI)

### Prerequisites

- Docker (for LocalStack)
- `awslocal` via localstack-cli
- wasmtime, wac-cli

### Build & Compose

```bash
# From repository root:

# Build vfs-adapter with S3 sync
cargo build -p vfs-adapter --target wasm32-wasip2 --features s3-sync

# Build the demo app
cargo build -p static-s3-demo --target wasm32-wasip2

# Compose with wac
wac plug \
    --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
    target/wasm32-wasip2/debug/static-s3-demo.wasm \
    -o /tmp/static-s3-composed.wasm
```

## Comparison with Other Approaches

| Approach | S3 Sync | Multi-process | Complexity |
|----------|---------|---------------|------------|
| Static (this) | Yes | No | Low |
| Dynamic (runtime-linker-s3) | Yes | No | Medium |
| RPC (vfs-rpc-server) | Yes | Yes | High |

Use **static composition** when:
- Single WASM component needs S3 persistence
- Simplest deployment (one file)
- No need for multi-process VFS sharing
