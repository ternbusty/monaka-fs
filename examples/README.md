# Examples

Examples demonstrating the Halycon VFS.

## Apps

| Directory | Description |
|-----------|-------------|
| [apps/](./apps/) | Deployment-agnostic WASM apps (demo-fs-operations, demo-writer, demo-reader) |

## Deployment Methods

| Method | Directory | Description |
|--------|-----------|-------------|
| [Static Composition](./static-composition/) | `static-composition/` | Build-time composition with `wac plug` + `vfs-adapter` |
| [Host Trait](./host-trait/) | `host-trait/` | Native host binary using `vfs-host` for runtime dynamic linking |
| [RPC Server](./rpc-server/) | `rpc-server/` | TCP-based sharing via `vfs-rpc-server` on port 9000 |

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
```

See each subdirectory's README for details.
