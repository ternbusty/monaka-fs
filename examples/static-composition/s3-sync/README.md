# Static Composition + S3 Sync Demo

This example demonstrates using VFS with S3 synchronization through **static composition** (`wac plug`).

Unlike the RPC approach (separate server process), this embeds the VFS + S3 sync directly into the WASM component.

## Prerequisites

- Docker (for LocalStack)
- AWS CLI (`awslocal` via localstack-cli)
- wasmtime
- wac-cli

## Architecture

```
┌─────────────────────────────────────────────┐
│           Composed WASM Component           │
│  ┌─────────────┐    ┌───────────────────┐   │
│  │  App (demo) │───>│    vfs-adapter    │   │
│  └─────────────┘    │   + s3-sync       │   │
│                     │                   │   │
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

## Build

All commands from the repository root.

```bash
# Build vfs-adapter with S3 sync
cargo build -p vfs-adapter --target wasm32-wasip2 --features s3-sync

# Build this demo app
cargo build --manifest-path examples/static-composition/s3-sync/Cargo.toml --target wasm32-wasip2

# Compose with wac
wac plug \
    --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
    examples/static-composition/s3-sync/target/wasm32-wasip2/debug/static-s3-demo.wasm \
    -o target/wasm32-wasip2/debug/static-s3-composed.wasm
```

## Start LocalStack

From the repository root:

```bash
docker compose up -d
```

This creates the `test-vfs-bucket` bucket automatically via the init script.

## Run

```bash
wasmtime run -S inherit-network=y -S http \
    --env VFS_S3_BUCKET=test-vfs-bucket \
    --env VFS_S3_PREFIX=demo/ \
    --env VFS_SYNC_MODE=realtime \
    --env AWS_ENDPOINT_URL=http://localhost:4566 \
    --env AWS_ACCESS_KEY_ID=test \
    --env AWS_SECRET_ACCESS_KEY=test \
    --env AWS_REGION=ap-northeast-1 \
    target/wasm32-wasip2/debug/static-s3-composed.wasm
```

## Verify S3 Sync

```bash
# List synced files
awslocal s3 ls s3://test-vfs-bucket/demo/ --recursive

# Read synced content
awslocal s3 cp s3://test-vfs-bucket/demo/files/data/config.json -
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
