# Example Apps

Deployment-agnostic WASM applications that only use `std::fs` APIs, unaware of the underlying VFS implementation.

Can be combined with any deployment method:
- [Static Composition](../static-composition/) — compose with `vfs-adapter` using `wac plug`
- [Host Trait](../host-trait/) — load from a native host via `vfs-host`
- [RPC Server](../rpc-server/) — compose with `rpc-adapter` using `wac plug`, connect to `vfs-rpc-server` over TCP

## Apps

| App | Description |
|-----|-------------|
| [demo-fs-operations/](./demo-fs-operations/) | Comprehensive `std::fs` tests (read, write, append, metadata, directory, error handling) |
| [demo-writer/](./demo-writer/) | Writes a file. Used with demo-reader to verify cross-process VFS sharing |
| [demo-reader/](./demo-reader/) | Reads a file written by demo-writer |

## Build

```bash
# From repository root
cargo build -p demo-fs-operations --target wasm32-wasip2
cargo build -p demo-writer --target wasm32-wasip2
cargo build -p demo-reader --target wasm32-wasip2
```

Output: `target/wasm32-wasip2/debug/{demo-fs-operations,demo-writer,demo-reader}.wasm`
