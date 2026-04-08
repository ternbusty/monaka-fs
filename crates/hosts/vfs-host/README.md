# vfs-host

Host trait implementation that enables multiple WASM applications to share a single in memory filesystem at runtime.

## Overview

`vfs-host` implements WASI filesystem Host traits using `fs-core` directly. Multiple applications can concurrently access and modify the same VFS instance through `Arc<Fs>`. Thread safety is handled internally by `fs-core` via `DashMap`, so no external locking is required.

## Features

* Shared VFS state across multiple applications via `Arc<Fs>`
* Thread safe without external locks
* VFS state persists as long as any application holds a reference
* Full WASI Preview 2 filesystem trait implementation
* Stream API support (`read_via_stream`, `write_via_stream`, `append_via_stream`)
* Optional S3 synchronization via the `s3-sync` feature

## Usage

### Basic

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

### Sharing VFS across multiple applications

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
                       Arc<Fs>
                           │
                   ┌───────┴───────┐
                   │    fs-core    │
                   │  (In Memory)  │
                   └───────────────┘
```

## S3 Sync

Enable the `s3-sync` feature to synchronize the in memory filesystem with S3.

```rust
let vfs_host = VfsHostState::new_with_s3(bucket, prefix).await?;
```

## Example

See `examples/host-trait/runtime-linker` for a complete working example.

## License

MIT
