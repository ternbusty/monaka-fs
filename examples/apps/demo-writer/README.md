# demo-writer

Writes content to a file at the specified path. Used together with demo-reader to verify cross-process VFS sharing.

```
Usage: demo-writer <path> <content>
```

## Build

```bash
# From repository root:
cargo build -p demo-writer --target wasm32-wasip2
```

## Usage

Combine it with one of the deployment methods:

- **Host Trait**: runtime-linker loads demo-writer then demo-reader, verifying shared VFS
- **RPC Server**: `wac plug --plug rpc_adapter.wasm demo-writer.wasm -o composed.wasm`, then `wasmtime run -S inherit-network=y composed.wasm /message.txt "Hello"` with server running

See [host-trait](../../host-trait/), [rpc-server](../../rpc-server/) for details.
