# RPC Adapter Implementation Plan

## Summary
Created RPC adapter architecture to enable std::fs to work transparently over RPC, following the vfs-host pattern.

## What I've Done
1. Created `wit/rpc-adapter.wit` - World definition for RPC adapter component
2. Created `rpc-adapter/Cargo.toml` - Package configuration
3. Implemented rpc-adapter/src/lib.rs with RPC client structure
4. Fixed WIT package name and doc comment issues

## The Problem
wasmtime-wasi's `add_to_linker_sync` bypasses custom Host trait implementations for preopens, directly accessing `WasiCtx.preopens` instead of calling our `get_directories()` method.

## The Solution
Follow vfs-host's architecture:
1. **rpc-adapter.wasm** - WASM component that exports wasi:filesystem and makes RPC calls
2. **vfs-rpc-host** - Wraps rpc-adapter component with Host traits (like vfs-host wraps vfs-adapter)

## Architecture

```
std::fs → WASI → vfs-rpc-host → rpc-adapter.wasm → TCP RPC → vfs-rpc-server → fs-core
```

## Current Status - ARCHITECTURAL ISSUE DISCOVERED

### Completed Steps
1. **rpc-adapter WASM component** - Successfully built (5.1MB)
   - Exports wasi:filesystem/preopens and wasi:filesystem/types
   - get_directories() returns root directory "/"
   - Connects to vfs-rpc-server via TCP on localhost:9000
   - Build succeeds without errors

2. **vfs-rpc-host library** - Successfully built
   - Wraps rpc-adapter component with Host traits
   - Implements filesystem_preopens::Host and filesystem_types::Host
   - Maps resources between host and component
   - Build succeeds with 1 warning (unused variable)

3. **rpc-fs-runner** - Successfully built
   - Host program that loads rpc-adapter and runs WASM apps
   - Uses VfsRpcHostState as store data
   - Builds and runs successfully

### Discovered Problem: wasmtime-wasi 27 Preopens Architecture

When testing demo-std-fs with rpc-fs-runner:
- **Error**: "failed to find a pre-opened file descriptor through which "test.txt" could be opened"
- **Debug output**: VfsRpcHostState::get_directories() is NEVER called
- **Root cause**: wasmtime-wasi 27 bypasses custom Host trait implementations for preopens
- **Why**: wasmtime-wasi directly reads WasiCtx.preopens field instead of calling Host::get_directories()

### Architecture Limitation

The current approach has a fundamental limitation:
- demo-std-fs expects wasmtime-wasi's standard preopens mechanism
- wasmtime-wasi 27 requires preopens to be set in WasiCtx.preopens field via WasiCtxBuilder
- Custom Host trait implementations are called AFTER WasiCtx.preopens lookup fails
- Our VfsRpcHostState.wasi_ctx has no preopens configured (intentionally, to delegate to rpc-adapter)
- Therefore, file operations fail before reaching our Host trait code

### Remaining Work (Architectural Decision Required)
- **Option A**: Use WASM Component Composition
  - Compose demo-std-fs with rpc-adapter using `wasm-tools compose`
  - This makes demo-std-fs directly import from rpc-adapter
  - More aligned with WASI Preview 2 component model

- **Option B**: Manually populate WasiCtx.preopens
  - Use WasiCtxBuilder to add a virtual directory
  - Requires creating Dir objects that forward to rpc-adapter
  - More complex but works within wasmtime-wasi architecture

- **Option C**: Continue with direct RPC applications
  - Apps like demo-app1/demo-app2 directly use RPC protocol
  - Don't try to make std::fs work transparently
  - This already works successfully

### Alternative Approaches to Consider
If TCP networking proves too complex for a component:
1. Use WASI Preview 1 (wasm32-wasi) target instead of Preview 2
2. Implement at host level only (skip component layer)
3. Use component composition with networking provided by host

## Next Steps
1. Resolve TCP socket API issues
2. Complete rpc-adapter implementation
3. Build as WASM: `cargo build -p rpc-adapter --target wasm32-wasip2`
4. Update vfs-rpc-host to wrap rpc-adapter (like vfs-host does)
5. Test end-to-end with demo-std-fs
