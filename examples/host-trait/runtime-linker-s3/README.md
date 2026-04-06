# Host Trait: Runtime Linker with S3 Sync

Same as `runtime-linker`, but files written to the VFS are automatically synced to S3 via `vfs-host`'s `s3-sync` feature.

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wasmtime-cli
```

Requires an S3-compatible endpoint. Start LocalStack from the repository root:

```bash
docker compose up -d
```

This creates the `test-vfs-bucket` bucket automatically via the init script.

## Build

```bash
# From repository root:

# Build the WASM app that will be loaded (see examples/apps/)
cargo build -p demo-writer --target wasm32-wasip2

# Build the host binary
cargo build -p runtime-linker-s3
```

## Run

```bash
# From repository root:
VFS_S3_BUCKET=test-vfs-bucket \
AWS_ENDPOINT_URL=http://localhost:4566 \
AWS_ACCESS_KEY_ID=test \
AWS_SECRET_ACCESS_KEY=test \
AWS_REGION=ap-northeast-1 \
cargo run -p runtime-linker-s3
```

## Verify S3 Sync

```bash
awslocal s3 ls s3://test-vfs-bucket/vfs/files/ --recursive
```

Expected:

```
<date> <time>         16 vfs/files/message.txt
```

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `VFS_S3_BUCKET` | Yes | - | S3 bucket name |
| `VFS_S3_PREFIX` | No | `vfs/` | Key prefix in bucket |
| `VFS_SYNC_MODE` | No | `batch` | `batch` or `realtime` |
| `AWS_ENDPOINT_URL` | No | - | For LocalStack/MinIO |
| `AWS_ACCESS_KEY_ID` | Yes | - | AWS credential |
| `AWS_SECRET_ACCESS_KEY` | Yes | - | AWS credential |
| `AWS_REGION` | No | - | AWS region |
