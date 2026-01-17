# vfs-host

Host trait implementation for VFS that enables multiple WebAssembly applications to share a single in-memory filesystem at runtime.

## Overview

`vfs-host` is a Rust library that implements wasmtime Host traits using fs-core directly. This library enables multiple applications to concurrently access and modify the same VFS instance.

## Features

- Shared VFS State: Multiple applications access the same VFS instance concurrently
- Thread-Safe: Uses `Arc<Mutex<>>` for safe concurrent access
- State Persistence: VFS state persists as long as any application references it
- Direct fs-core Integration: No WASM adapter needed, uses fs-core natively
- Complete WASI Implementation: All 33 WASI filesystem Host trait methods implemented
  - 26 real implementations (file I/O, directories, metadata, stream API, etc.)
  - 7 stub implementations (advisory hints, sync operations)
- Full Stream API Support: Complete implementation of `read_via_stream`, `write_via_stream`, `append_via_stream`
- Zero-Copy Resource Mapping: Efficient descriptor and stream resource management

## Usage

### Basic Example

```rust
use vfs_host::VfsHostState;
use wasmtime::{Engine, Store, Config};

// Create engine with component model support
let mut config = Config::new();
config.wasm_component_model(true);
let engine = Engine::new(&config)?;

// Create VFS host (uses fs-core directly, no WASM adapter needed)
let vfs_host = VfsHostState::new()?;

// Create application store
let mut store = Store::new(&engine, vfs_host);
```

### Sharing VFS Across Multiple Applications

```rust
use vfs_host::VfsHostState;
use wasmtime::{Engine, Store};

// Create shared VFS host
let vfs_host = VfsHostState::new()?;

// Create multiple application contexts sharing the same VFS
let vfs_host2 = vfs_host.clone_shared();
let vfs_host3 = vfs_host.clone_shared();

// Create stores for each application
let mut store1 = Store::new(&engine, vfs_host);
let mut store2 = Store::new(&engine, vfs_host2);
let mut store3 = Store::new(&engine, vfs_host3);

// All three stores share the same VFS instance
```

### State Persistence Example

```rust
use vfs_host::VfsHostState;

let vfs_host = VfsHostState::new()?;
let vfs_host2 = vfs_host.clone_shared();

{
    // App1 creates data
    let mut store1 = Store::new(&engine, vfs_host);
    // ... App1 creates files/directories ...

    // Store1 is dropped here
}

// App2 starts AFTER App1 terminated
let mut store2 = Store::new(&engine, vfs_host2);
// App2 can still access files created by App1
// VFS state persisted because vfs_host2 still held a reference
```

## Architecture

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ Application1 │    │ Application2 │    │ Application3 │
└──────┬───────┘    └──────┬───────┘    └──────┬───────┘
       │                   │                   │
       │    VfsHostState (clone_shared())      │
       └───────────────────┼───────────────────┘
                           │
                  Arc<Mutex<SharedVfsCore>>
                           │
                   ┌───────┴───────┐
                   │   fs-core     │
                   │  (In-Memory)  │
                   └───────────────┘
```

## API Reference

### Core Types

#### `VfsHostState`

Main host context that implements WASI Host traits.

```rust
pub struct VfsHostState {
    pub wasi_ctx: WasiCtx,
    pub table: ResourceTable,
    pub shared_vfs: Arc<Mutex<SharedVfsCore>>,
}
```

**Methods:**

- `new() -> Result<Self>`
  - Creates a new VFS host with a fresh fs-core filesystem instance

- `clone_shared(&self) -> Self`
  - Creates a new VFS host that shares the same VFS core
  - Used to create multiple application contexts sharing one VFS

- `clone_shared_with_env(&self, env_vars: &[(&str, &str)]) -> Self`
  - Creates a new VFS host with shared VFS and custom environment variables

- `from_shared_vfs_with_env(shared_vfs: Arc<Mutex<SharedVfsCore>>, env_vars: &[(&str, &str)]) -> Self`
  - Creates a new VFS host from an existing shared VFS with environment variables

- `get_shared_vfs(&self) -> Arc<Mutex<SharedVfsCore>>`
  - Returns the shared VFS core for external use

#### `SharedVfsCore`

Shared VFS state accessed by all applications.

```rust
pub struct SharedVfsCore {
    pub fs: Fs,  // fs-core filesystem instance
}
```

## Implementation Details

### Host Trait Coverage

The library implements all WASI Preview 2 filesystem Host traits:

wasi:filesystem/types@0.2.6:
- `Host` trait (2 methods): Error conversion utilities
- `HostDescriptor` trait (28 methods): File/directory operations
- `HostDirectoryEntryStream` trait (2 methods): Directory listing

wasi:filesystem/preopens@0.2.6:
- `Host` trait (1 method): Preopened directory listing

### Real Implementations (26 methods)

- File I/O: `read`, `write`
- Path operations: `open_at`, `stat`, `stat_at`, `read_directory`
- Directory ops: `create_directory_at`, `remove_directory_at`, `unlink_file_at`
- Metadata ops: `set_size`, `get_flags`, `get_type`
- Comparison ops: `is_same_object`, `metadata_hash`, `metadata_hash_at`
- Stream API: `read_via_stream`, `write_via_stream`, `append_via_stream`
- Directory streaming: `read_directory_entry`, `drop` (DirectoryEntryStream)

### Unsupported Operations

These methods return `Unsupported` error as fs-core doesn't support them:
- `link_at`, `symlink_at`, `readlink_at`: Symbolic/hard links
- `rename_at`: File renaming
- `set_times`, `set_times_at`: Timestamp modification
- `advise`, `sync_data`, `sync`: Advisory/sync operations (no-op for in-memory FS)

## Complete Example

See the `examples/component-model/runtime-linker` directory in the parent repository for a complete working example:

```bash
cd examples/component-model/runtime-linker
cargo run
```

## Dependencies

- `wasmtime` v27.0: WebAssembly runtime with component model support
- `wasmtime-wasi` v27.0: WASI host implementations
- `fs-core`: In-memory filesystem implementation
- `anyhow` v1.0: Error handling

## License

MIT OR Apache-2.0
