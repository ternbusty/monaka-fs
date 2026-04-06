# CI Cache Demo

Multiple parallel CI jobs sharing a dependency cache via VFS RPC server. Jobs acquire per-library locks to ensure safe concurrent access.

**Deployment method**: RPC Server (`vfs-rpc-server`)

```
Job1 (serde, tokio)  --+
Job2 (serde, anyhow) --+--> vfs-rpc-server (TCP:9000) --> /cache/
Job3 (tokio, anyhow) --+
```

## Using `monaka` CLI

```bash
# Build the app
cargo build -p ci-job --target wasm32-wasip2

# Compose with RPC adapter
make build-cli
target/release/monaka compose --rpc \
  target/wasm32-wasip2/debug/ci-job.wasm \
  -o /tmp/ci-job-composed.wasm

# Extract and start the RPC server
target/release/monaka  extract server -o /tmp/vfs-rpc-server.wasm
wasmtime run -S inherit-network=y -S http /tmp/vfs-rpc-server.wasm

# In another terminal: run 3 jobs in parallel
wasmtime run -S inherit-network=y --env JOB_ID=1 --env DEPS="serde-1.0.0,tokio-1.0.0" /tmp/ci-job-composed.wasm &
wasmtime run -S inherit-network=y --env JOB_ID=2 --env DEPS="serde-1.0.0,anyhow-1.0.0" /tmp/ci-job-composed.wasm &
wasmtime run -S inherit-network=y --env JOB_ID=3 --env DEPS="tokio-1.0.0,anyhow-1.0.0" /tmp/ci-job-composed.wasm &
wait
```

## Locking Protocol

For each dependency:
1. **Acquire lock**: `mkdir /cache/{lib}.lock` (atomic, fails if exists)
2. **Check cache**: Read `/cache/{lib}.cache` if exists
3. **On miss**: Simulate download, write cache file
4. **Release lock**: `rmdir /cache/{lib}.lock`

## Expected Output

```
[Job1] Starting with deps: serde-1.0.0, tokio-1.0.0
[Job2] Starting with deps: serde-1.0.0, anyhow-1.0.0
[Job3] Starting with deps: tokio-1.0.0, anyhow-1.0.0

[Job1] serde-1.0.0: acquiring lock...
[Job1] serde-1.0.0: lock acquired
[Job1] serde-1.0.0: MISS - downloading...
...
[Job2] serde-1.0.0: HIT (52 bytes)
...
[Job1] Done
[Job2] Done
[Job3] Done
```

## Manual Setup (without `monaka` CLI)

```bash
cargo build -p vfs-rpc-server --target wasm32-wasip2
cargo build -p rpc-adapter --target wasm32-wasip2
cargo build -p ci-job --target wasm32-wasip2

wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/ci-job.wasm \
  -o /tmp/ci-job-composed.wasm
```
