# demo-fs-operations

Comprehensive `std::fs` test application. Verifies the following operations:

1. **Basic File Operations** — write, read, append, delete
2. **Directory Operations** — create_dir, create_dir_all, read_dir, remove_dir
3. **Metadata Operations** — metadata, truncate
4. **Error Handling** — non-existent files, duplicate directories, directory-as-file

## Build

```bash
# From repository root:
cargo build -p demo-fs-operations --target wasm32-wasip2
```

## Usage

Combine with one of the deployment methods to use the in-memory VFS:

- **Static Composition**: `wac plug --plug vfs_adapter.wasm demo-fs-operations.wasm -o composed.wasm && wasmtime run composed.wasm`
- **Host Trait**: load as a WASM component from runtime-linker
- **RPC Server**: `wac plug --plug rpc_adapter.wasm demo-fs-operations.wasm -o composed.wasm`, then `wasmtime run -S inherit-network=y composed.wasm` with server running

See [static-composition](../../static-composition/), [host-trait](../../host-trait/), [rpc-server](../../rpc-server/) for details.
