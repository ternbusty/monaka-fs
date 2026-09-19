# Static Composition + S3 Sync Demo

VFS with S3 synchronization through static composition. The VFS + S3 sync is embedded directly into the WASM component.

## Using `monaka` CLI

```bash
# Build the demo app (standalone package)
cd examples/static-composition/s3-sync && cargo build --target wasm32-wasip2 && cd ../../..

# Compose with S3 sync adapter
make build-cli
target/release/monaka compose --s3-sync \
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
| `VFS_S3_FILE_LOCK` | `enabled` or `disabled`. Per-file S3 lease on open for write | `enabled` |
| `VFS_S3_FILE_LOCK_TIMEOUT_MS` | Wait for a lease held by another instance | `10000` |
| `VFS_S3_FILE_LOCK_LEASE_SECS` | Lease lifetime, only relevant if a holder crashes | `30` |
| `AWS_ENDPOINT_URL` | S3 endpoint (for LocalStack) | - |
| `AWS_ACCESS_KEY_ID` | AWS credential | - |
| `AWS_SECRET_ACCESS_KEY` | AWS credential | - |
| `AWS_REGION` | AWS region | - |

## Manual Setup (without `monaka` CLI)

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

| Approach | S3 Sync | Shared VFS in one process group | Several processes on one bucket | Complexity |
|----------|---------|---------------------------------|--------------------------------|------------|
| Static (this) | Yes | No | Yes, through S3 leases | Low |
| Dynamic (runtime-linker-s3) | Yes | Yes | Yes, through S3 leases | Medium |
| RPC (vfs-rpc-server) | Yes | Yes | Yes, through S3 leases | High |

Any number of composed processes may share one bucket and prefix. Writes to the same file are serialized by the per-file lease described in [docs/sync-semantics.md](../../../docs/sync-semantics.md).

Use **static composition** when:
- Single WASM component needs S3 persistence
- Simplest deployment (one file)
- No need for multi-process VFS sharing
