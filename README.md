# MonakaFS

An in-memory filesystem for WebAssembly. Multiple WASM apps can share a virtual filesystem transparently using standard `std::fs` APIs.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/ternbusty/monaka-fs/main/scripts/install.sh | bash
```

Or build from source:

```bash
make build-cli
```

## Quick Start

### Static Composition (single WASM binary)

Compose your app with the built-in VFS adapter into a single file:

```bash
monaka compose my-app.wasm -o composed.wasm
wasmtime run composed.wasm
```

With file embedding:

```bash
monaka compose --mount /data=./local-dir my-app.wasm -o composed.wasm
wasmtime run composed.wasm
```

With S3 sync (files are automatically persisted to S3):

```bash
monaka compose --s3-sync my-app.wasm -o composed.wasm

wasmtime run -S inherit-network=y -S http \
  --env VFS_S3_BUCKET=my-bucket \
  --env VFS_S3_PREFIX=vfs/ \
  --env VFS_SYNC_MODE=realtime \
  --env AWS_ENDPOINT_URL=https://s3.amazonaws.com \
  --env AWS_ACCESS_KEY_ID=... \
  --env AWS_SECRET_ACCESS_KEY=... \
  --env AWS_REGION=ap-northeast-1 \
  composed.wasm
```

### RPC (multi-process shared VFS)

Multiple apps share a single VFS instance over TCP:

```bash
# Compose your app with the RPC adapter
monaka compose --rpc my-app.wasm -o composed.wasm

# Start the VFS server
monaka extract server -o vfs-rpc-server.wasm
wasmtime run -S inherit-network=y vfs-rpc-server.wasm

# Run your app (in another terminal)
wasmtime run -S inherit-network=y composed.wasm
```

To persist the shared VFS to S3, use `--s3-sync` for the server:

```bash
monaka extract server --s3-sync -o vfs-rpc-server.wasm
wasmtime run -S inherit-network=y -S http \
  --env VFS_S3_BUCKET=my-bucket \
  --env VFS_S3_PREFIX=vfs/ \
  --env VFS_SYNC_MODE=realtime \
  --env AWS_ENDPOINT_URL=https://s3.amazonaws.com \
  --env AWS_ACCESS_KEY_ID=... \
  --env AWS_SECRET_ACCESS_KEY=... \
  --env AWS_REGION=ap-northeast-1 \
  vfs-rpc-server.wasm
```

### Host Trait (Rust library)

For wasmtime host programs, add the `vfs-host` crate:

```bash
cargo add vfs-host
```

```rust
use vfs_host::VfsHostState;

let state = VfsHostState::new()?;
// Use with wasmtime Store to provide VFS to WASM guests. See: examples/host-trait/runtime-linker
```

With S3 sync:

```bash
cargo add vfs-host -F s3-sync
```

```rust
// See: examples/host-trait/runtime-linker-s3
use vfs_host::VfsHostState;

let state = VfsHostState::new_with_s3(
    "my-bucket".to_string(),
    "vfs/".to_string(),
).await?;
```

## S3 Sync Configuration

All three deployment models support S3 synchronization. Behavior is controlled by environment variables:

| Variable | Values | Default | Description |
|----------|--------|---------|-------------|
| `VFS_S3_BUCKET` | | (required) | S3 bucket name |
| `VFS_S3_PREFIX` | | `vfs/` | Key prefix for synced files |
| `VFS_SYNC_MODE` | `batch`, `realtime` | `batch` | `batch`: periodic flush. `realtime`: immediate S3 PUT on every write |
| `VFS_READ_MODE` | `memory`, `s3` | `memory` | `memory`: read from VFS only. `s3`: read-through from S3 on every read |
| `VFS_METADATA_MODE` | `local`, `s3` | `local` | `local`: use cached metadata. `s3`: HEAD request on every file open |
| `AWS_ACCESS_KEY_ID` | | | AWS credential |
| `AWS_SECRET_ACCESS_KEY` | | | AWS credential |
| `AWS_REGION` | | | AWS region |
| `AWS_ENDPOINT_URL` | | | Custom endpoint (for LocalStack, GCS, MinIO, etc.) |

For Static Composition and RPC, these are passed as `--env` flags to `wasmtime run`. For Host Trait, these are read from the process environment by the AWS SDK.

## Three Deployment Models

| Model | Use Case | How |
|-------|----------|-----|
| **Static Composition** | Single app, simplest deployment | `monaka compose app.wasm -o out.wasm` |
| **RPC** | Multiple apps sharing filesystem | `monaka compose --rpc` + `monaka extract server` |
| **Host Trait** | Custom wasmtime host in Rust | `cargo add vfs-host` |

Each model supports optional S3 synchronization for cloud persistence.

## CLI Commands

| Command | Description |
|---------|-------------|
| `monaka compose` | Compose an app with a bundled adapter |
| `monaka embed` | Embed files into the bundled vfs-adapter |
| `monaka extract` | Extract a bundled binary (adapter, rpc-adapter, server) |

Run `monaka --help` for details.

## Examples & Use Cases

- [Static Composition examples](./examples/static-composition/) — file embedding, C apps, S3 sync
- [RPC Server examples](./examples/rpc-server/) — multi-app shared VFS
- [Host Trait examples](./examples/host-trait/) — native wasmtime host programs
- [Use Cases](./usecases/) — sensor pipeline, HTTP cache, CI cache, image pipeline, and more

## For Developers

### Project Structure

```
crates/
├── core/fs-core/           # Core filesystem (no_std, all FS operations)
├── adapters/
│   ├── vfs-adapter/        # WASI Component Model adapter
│   └── rpc-adapter/        # RPC client adapter
├── hosts/
│   ├── vfs-host/           # wasmtime host for dynamic linking
│   └── vfs-rpc-host/       # wasmtime host for RPC-based access
├── sync/
│   ├── vfs-sync-core/      # Core sync types
│   ├── vfs-sync-host/      # S3 sync for vfs-host / vfs-rpc-server
│   └── vfs-sync-adapter/   # S3 sync for vfs-adapter (WASI)
├── rpc/
│   ├── vfs-rpc-protocol/   # RPC message types
│   └── vfs-rpc-server/     # TCP server on port 9000
└── tools/
    └── monaka-cli/        # Monaka CLI (embed, compose, extract)
```

### Building

```bash
cargo build                   # Build native packages
make build-wasm               # Build all WASM components
make build-cli                # Build WASM + CLI
cargo test                    # Run all tests
```

### Git Hooks

Pre-commit checks via [lefthook](https://github.com/evilmartians/lefthook):

```bash
lefthook install
```

## License

MIT
