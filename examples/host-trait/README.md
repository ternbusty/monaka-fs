# Host Trait Examples

Runtime dynamic linking using `vfs-host` — a native host binary that loads WASM applications and provides the VFS via Wasmtime's `Linker` API.

```
Native Host (vfs-host)
├── Load demo-writer.wasm → VfsHostState (shared)
└── Load demo-reader.wasm → VfsHostState (clone_shared)
```

Multiple WASM apps share the same in-memory filesystem through `VfsHostState::clone_shared()`.
Loads apps from [apps/](../apps/) as WASM components.

## Examples

| Directory | Description |
|-----------|-------------|
| [runtime-linker/](./runtime-linker/) | Basic runtime dynamic linking |
| [runtime-linker-s3/](./runtime-linker-s3/) | Runtime dynamic linking with S3 sync |

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wasmtime-cli
```

See each subdirectory's README for build, run, and expected output details.
