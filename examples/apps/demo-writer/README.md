# demo-writer

Writes a file to `/message.txt`. Used together with demo-reader to verify cross-process VFS sharing.

## Build

```bash
# From repository root:
cargo build -p demo-writer --target wasm32-wasip2
```

## Usage

Combine it with one of the deployment methods:

- **Host Trait**: runtime-linker loads demo-writer then demo-reader, verifying shared VFS
- **RPC Server**: `wac plug --plug rpc_adapter.wasm demo-writer.wasm -o composed.wasm`, then `wasmtime run -S inherit-network=y composed.wasm` with server running

See [host-trait](../../host-trait/), [rpc-server](../../rpc-server/) for details.
