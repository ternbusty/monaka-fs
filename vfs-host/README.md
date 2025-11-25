# vfs-host

Host trait implementation for VFS adapter - enables multiple WebAssembly applications to share a single VFS instance at runtime.

## Overview

`vfs-host` is a Rust library that implements wasmtime Host traits for the VFS adapter component. Unlike traditional approaches where each application gets an isolated filesystem (like `wasi-virt`), this library enables **true shared filesystem semantics** where multiple applications can concurrently access and modify the same VFS instance.

## Key Features

- **Shared VFS State**: Multiple applications access the same VFS instance concurrently
- **Thread-Safe**: Uses `Arc<Mutex<>>` for safe concurrent access
- **State Persistence**: VFS state persists as long as any application references it
- **Complete WASI Implementation**: All 33 WASI filesystem Host trait methods implemented
  - 26 real implementations (file I/O, directories, metadata, stream API, etc.)
  - 7 stub implementations (advisory hints, sync operations)
- **Full Stream API Support**: Complete implementation of `read_via_stream`, `write_via_stream`, `append_via_stream`
- **Zero-Copy Resource Mapping**: Efficient descriptor and stream resource management

## When to Use

Use `vfs-host` when you need:

1. **Multiple applications sharing the same filesystem**
   - App1 creates a file → App2 can read it immediately
   - Changes are visible across all applications in real-time

2. **Persistent filesystem state**
   - VFS state persists even after individual applications terminate
   - State is kept alive as long as any application holds a reference

3. **Runtime flexibility**
   - Swap VFS implementations at runtime
   - Independent component updates without rebuilding

4. **Custom host implementations**
   - Build your own WebAssembly host with integrated VFS
   - Full control over resource management

## Usage

### Basic Example

```rust
use vfs_host::VfsHostState;
use wasmtime::{Engine, Store, Config};

// Create engine with component model support
let mut config = Config::new();
config.wasm_component_model(true);
let engine = Engine::new(&config)?;

// Create VFS host (loads VFS adapter component)
let vfs_host = VfsHostState::new(&engine, "path/to/vfs-adapter.wasm")?;

// Create application store
let mut store = Store::new(&engine, vfs_host);

// Now your store implements all WASI filesystem Host traits!
// Applications loaded in this store can use the filesystem
```

### Sharing VFS Across Multiple Applications

```rust
use vfs_host::VfsHostState;
use wasmtime::{Engine, Store};

// Create shared VFS host
let vfs_host = VfsHostState::new(&engine, "vfs-adapter.wasm")?;

// Create multiple application contexts sharing the same VFS
let vfs_host2 = vfs_host.clone_shared();
let vfs_host3 = vfs_host.clone_shared();

// Create stores for each application
let mut store1 = Store::new(&engine, vfs_host);
let mut store2 = Store::new(&engine, vfs_host2);
let mut store3 = Store::new(&engine, vfs_host3);

// All three stores share the same VFS instance!
// Changes made by App1 are immediately visible to App2 and App3
```

### State Persistence Example

```rust
use vfs_host::VfsHostState;

let vfs_host = VfsHostState::new(&engine, "vfs-adapter.wasm")?;
let vfs_host2 = vfs_host.clone_shared();

{
    // App1 creates data
    let mut store1 = Store::new(&engine, vfs_host);
    // ... App1 creates files/directories ...

    // Store1 is dropped here
}

// App2 starts AFTER App1 terminated
let mut store2 = Store::new(&engine, vfs_host2);
// App2 can still access files created by App1!
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
                   │  VFS Adapter  │
                   │  (Component)  │
                   └───────────────┘
                           │
                   ┌───────┴───────┐
                   │   fs-core     │
                   │ (In-Memory)   │
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

- `new(engine: &Engine, vfs_adapter_path: &str) -> Result<Self>`
  - Creates a new VFS host by loading and instantiating the VFS adapter component

- `clone_shared(&self) -> Self`
  - Creates a new VFS host that shares the same VFS core
  - Used to create multiple application contexts sharing one VFS

#### `SharedVfsCore`

Shared VFS state accessed by all applications.

```rust
pub struct SharedVfsCore {
    pub vfs_instance: VfsAdapter,
    pub vfs_store: Arc<Mutex<Store<VfsStoreData>>>,
    pub descriptor_map: HashMap<u32, Descriptor>,
    pub dir_stream_map: HashMap<u32, DirectoryEntryStream>,
}
```

## Implementation Details

### Host Trait Coverage

The library implements all WASI Preview 2 filesystem Host traits:

**wasi:filesystem/types@0.2.6:**
- `Host` trait (2 methods) - Error conversion utilities
- `HostDescriptor` trait (28 methods) - File/directory operations
- `HostDirectoryEntryStream` trait (2 methods) - Directory listing

**wasi:filesystem/preopens@0.2.6:**
- `Host` trait (1 method) - Preopened directory listing

### Real Implementations (26 methods)

- **File I/O**: `read`, `write`
- **Path operations**: `open_at`, `stat`, `stat_at`, `read_directory`
- **Directory ops**: `create_directory_at`, `remove_directory_at`, `unlink_file_at`
- **Link ops**: `rename_at`, `link_at`, `symlink_at`, `readlink_at`
- **Metadata ops**: `set_size`, `set_times`, `set_times_at`, `get_flags`, `get_type`
- **Comparison ops**: `is_same_object`, `metadata_hash`, `metadata_hash_at`
- **Stream API**: `read_via_stream`, `write_via_stream`, `append_via_stream` - Full implementation
- **Directory streaming**: `read_directory_entry`, `drop` (DirectoryEntryStream)

### Stub Implementations (7 methods)

These methods return `Unsupported` error as they're not required for in-memory VFS:
- `advise`, `sync_data`, `sync` - Advisory/sync operations (no-op for in-memory FS)

## Comparison with Alternatives

| Approach | Shared State | Use Case | Complexity |
|----------|--------------|----------|------------|
| **vfs-host** (this library) | ✅ Yes | Multiple apps sharing VFS | Medium |
| **wac plug** (runtime composition) | ❌ No | Single app with VFS | Low |
| **wasi-virt** | ❌ No | Isolated VFS per app | Low |

## Complete Example

See the `examples/runtime-linker` directory in the parent repository for a complete working example:

```bash
cd examples/runtime-linker
cargo run
```

The example demonstrates:
- Creating shared VFS host state
- Multiple applications accessing the same VFS concurrently
- State persistence after application termination
- All filesystem operations working correctly

## Dependencies

- `wasmtime` v27.0 - WebAssembly runtime with component model support
- `wasmtime-wasi` v27.0 - WASI host implementations
- `anyhow` v1.0 - Error handling

## License

MIT OR Apache-2.0
