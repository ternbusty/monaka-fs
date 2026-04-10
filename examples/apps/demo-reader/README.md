# demo-reader

Reads a file at the specified path and prints its content to stdout. Used together with demo-writer to verify cross-process VFS sharing.

```
Usage: demo-reader <path>
```

## Build

```bash
# From repository root:
cargo build -p demo-reader --target wasm32-wasip2
```

## Usage

Combine it with one of the deployment methods:

- **Host Trait**: runtime-linker loads demo-writer then demo-reader, verifying shared VFS
- **RPC Server**: `wac plug --plug rpc_adapter.wasm demo-reader.wasm -o composed.wasm`, then `wasmtime run -S inherit-network=y composed.wasm /message.txt` with server running

See [host-trait](../../host-trait/), [rpc-server](../../rpc-server/) for details.
