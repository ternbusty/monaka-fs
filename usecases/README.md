# Use Cases

End-to-end demonstrations of Monaka VFS in realistic scenarios, organized by deployment method.

## Static Composition

Compose your app with `vfs-adapter` into a single WASM binary using the `monaka` CLI:

```bash
monaka compose my-app.wasm -o composed.wasm
```

| Use Case | Description |
|----------|-------------|
| [image-pipeline](./static-composition/image-pipeline/) | File-based image processing pipeline |

## Host Trait

Embed VFS directly into a native Rust host program using the `vfs-host` crate:

```bash
cargo add vfs-host
```

| Use Case | Description |
|----------|-------------|
| [sensor-pipeline](./host-trait/sensor-pipeline/) | Two WASM apps sharing sensor data via VFS |
| [http-cache](./host-trait/http-cache/) | HTTP server with shared VFS cache across WASM handlers |
| [concurrent-append](./host-trait/concurrent-append/) | Concurrent append test with native multithreading |

## RPC Server

Compose your app with `rpc-adapter` and run a shared VFS server:

```bash
monaka compose --rpc my-app.wasm -o composed.wasm
monaka extract server -o vfs-rpc-server.wasm
```

| Use Case | Description |
|----------|-------------|
| [ci-cache](./rpc-server/ci-cache/) | Parallel CI jobs sharing dependency cache |
| [s3-sync-logging](./rpc-server/s3-sync-logging/) | Replicated log writers with S3 sync |

See each subdirectory's README for build, run, and expected output details.
