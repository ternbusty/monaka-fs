# RPC Concurrent Append Test

Multiple WASM clients concurrently appending to the same file through the VFS RPC Server. Verifies that no data corruption occurs.

**Deployment method**: [RPC Server](../../examples/rpc-server/) (`vfs-rpc-server`)

```
Client 1 --+
Client 2 --+--> vfs-rpc-server (TCP:9000) --> /shared/concurrent.log
Client 3 --+
```

## Build

```bash
# From repository root:
cargo build -p vfs-rpc-server --target wasm32-wasip2
cargo build -p rpc-adapter --target wasm32-wasip2

# Build standalone packages
cd usecases/rpc-server/concurrent-append/append-client && cargo build --release --target wasm32-wasip2 && cd ../../../..
cd usecases/rpc-server/concurrent-append/verify-result && cargo build --release --target wasm32-wasip2 && cd ../../../..

# Compose
wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  usecases/rpc-server/concurrent-append/append-client/target/wasm32-wasip2/release/append-client.wasm \
  -o target/wasm32-wasip2/debug/composed-append-client.wasm

wac plug \
  --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
  usecases/rpc-server/concurrent-append/verify-result/target/wasm32-wasip2/release/verify-result.wasm \
  -o target/wasm32-wasip2/debug/composed-verify-result.wasm
```

## Run

Start the server (terminal 1):

```bash
# From repository root:
wasmtime run -S inherit-network=y -S http \
  target/wasm32-wasip2/debug/vfs_rpc_server.wasm
```

Run clients in parallel (terminal 2):

```bash
NUM_CLIENTS=3
APPEND_COUNT=50
PIDS=()

for i in $(seq 1 $NUM_CLIENTS); do
  wasmtime run -S inherit-network=y \
    --env CLIENT_ID=$i \
    --env APPEND_COUNT=$APPEND_COUNT \
    target/wasm32-wasip2/debug/composed-append-client.wasm &
  PIDS+=($!)
done

# Wait for clients only (not the background server)
for pid in "${PIDS[@]}"; do wait $pid; done
```

Verify results:

```bash
wasmtime run -S inherit-network=y \
  --env EXPECTED_CLIENTS=$NUM_CLIENTS \
  --env APPEND_COUNT=$APPEND_COUNT \
  target/wasm32-wasip2/debug/composed-verify-result.wasm
```

Stop the server:

```bash
pkill -f vfs_rpc_server.wasm
```

## Expected Output

```
=== Verification Result ===
PASS: All 150 lines verified, no data corruption

Concurrent append with proper locking: CONFIRMED
```
