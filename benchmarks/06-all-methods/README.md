# Benchmark 06: All VFS Methods Comparison

Compares the performance of three VFS implementation methods on macOS.

## Methods Compared

| Method | Description | Use Case |
|--------|-------------|----------|
| **Static Compose** | `wac plug` with `vfs-adapter` | Single WASM file, no runtime dependency |
| **Host Trait** | `wasmtime` with `vfs-host` | Native runtime, multiple apps share VFS |
| **RPC** | `rpc-adapter` + `vfs-rpc-server` | Network-based, cross-machine sharing |

## Running

```bash
cd benchmarks/06-all-methods
./run-bench.sh
```

## Architecture

```
Static Compose:
  [bench-app.wasm] --wac plug--> [vfs_adapter.wasm] --> wasmtime

Host Trait:
  [bench-app.wasm] --> wasmtime + vfs-host (native Host trait impl)

RPC:
  [bench-app.wasm] --wac plug--> [rpc_adapter.wasm] --> TCP:9000 --> [vfs-rpc-server]
```

## Build Components

### 1. Benchmark App (WASM)
```bash
cd bench-app
cargo build --release --target wasm32-wasip2
```

### 2. Adapters
```bash
cargo build --release --target wasm32-wasip2 -p vfs-adapter -p rpc-adapter -p vfs-rpc-server
```

### 3. Host Trait Runner (Native)
```bash
cd bench-runner
cargo build --release
```

### 4. Compose
```bash
wac plug --plug vfs_adapter.wasm bench-all-methods.wasm -o bench-static.wasm
wac plug --plug rpc_adapter.wasm bench-all-methods.wasm -o bench-rpc.wasm
```

## Expected Results

Typical performance order (fastest to slowest):
1. **Host Trait** - Direct fs-core calls, no WASM boundary crossing
2. **Static Compose** - Single WASM, fs-core in adapter
3. **RPC** - Network overhead for each operation

## Metrics

- **seq_write**: Sequential write throughput (MB/s)
- **seq_read**: Sequential read throughput (MB/s)
- **random_read**: Random read throughput (4KB blocks, 1MB file only)
