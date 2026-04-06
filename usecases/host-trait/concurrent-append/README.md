# Host Trait Concurrent Append Test

Multiple WASM instances running in parallel native threads, concurrently appending to the same file through shared VFS (`fs-core`). Validates fs-core's locking implementation (DashMap + per-inode RwLock).

**Deployment method**: [Host Trait](../../examples/host-trait/) (`vfs-host`)

```
host-runner (native Rust)
├── Thread 1 (Store 1, WASM) --+
├── Thread 2 (Store 2, WASM) --+--> fs-core (Arc<Fs>) --> /shared/concurrent.log
└── Thread 3 (Store 3, WASM) --+
```

Unlike the [RPC version](../../rpc-server/concurrent-append/), this uses true multithreaded parallelism.

## Build

```bash
# From repository root:

# Build append-client WASM (standalone package)
cd usecases/host-trait/concurrent-append/append-client && cargo build --release --target wasm32-wasip2 && cd ../../../..

# Build host-runner (standalone package)
cd usecases/host-trait/concurrent-append/host-runner && cargo build --release && cd ../../../..
```

## Run

```bash
# From repository root:
WASM_PATH=usecases/host-trait/concurrent-append/append-client/target/wasm32-wasip2/release/append-client.wasm \
  usecases/host-trait/concurrent-append/host-runner/target/release/host-concurrent-runner 3 50
```

Arguments: `host-concurrent-runner [num_clients] [append_count]`

## Expected Output

```
==============================================
  Host Trait Concurrent Append Test
==============================================

Configuration:
  Clients:         3
  Appends/client:  50
  Expected lines:  150

Starting 3 threads with shared VFS...
[Client 1] Completed: 50 success, 0 errors
[Client 2] Completed: 50 success, 0 errors
[Client 3] Completed: 50 success, 0 errors

--- Verification ---
Total lines:   150
Valid lines:   150
Invalid lines: 0

==============================================
  TEST PASSED
==============================================
```
