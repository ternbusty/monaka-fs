# CI Cache Demo

Multiple parallel CI jobs sharing a dependency cache via VFS RPC server. Jobs acquire per-library locks to ensure safe concurrent access.

**Deployment method**: [RPC Server](../../examples/rpc-server/) (`vfs-rpc-server`)

```
Job1 (serde, tokio)  --+
Job2 (serde, anyhow) --+--> vfs-rpc-server (TCP:9000) --> /cache/
Job3 (tokio, anyhow) --+
```

## Build

```bash
# From repository root:
cargo build -p vfs-rpc-server --target wasm32-wasip2
cargo build -p rpc-adapter --target wasm32-wasip2
cargo build -p ci-job --target wasm32-wasip2

# Compose
wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  target/wasm32-wasip2/debug/ci-job.wasm \
  -o target/wasm32-wasip2/debug/ci-job-composed.wasm
```

## Run

Start the server (terminal 1):

```bash
# From repository root:
wasmtime run -S inherit-network=y -S http \
  target/wasm32-wasip2/debug/vfs_rpc_server.wasm
```

Run 3 jobs in parallel (terminal 2):

```bash
wasmtime run -S inherit-network=y --env JOB_ID=1 --env DEPS="serde-1.0.0,tokio-1.0.0" \
  target/wasm32-wasip2/debug/ci-job-composed.wasm &

wasmtime run -S inherit-network=y --env JOB_ID=2 --env DEPS="serde-1.0.0,anyhow-1.0.0" \
  target/wasm32-wasip2/debug/ci-job-composed.wasm &

wasmtime run -S inherit-network=y --env JOB_ID=3 --env DEPS="tokio-1.0.0,anyhow-1.0.0" \
  target/wasm32-wasip2/debug/ci-job-composed.wasm &

wait
```

Stop the server:

```bash
pkill -f vfs_rpc_server.wasm
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
