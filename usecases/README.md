# Use Cases

End-to-end demonstrations of Halycon VFS in realistic scenarios, organized by deployment method.

## Static Composition

| Use Case | Description |
|----------|-------------|
| [image-pipeline](./static-composition/image-pipeline/) | File-based image processing pipeline |

## Host Trait

| Use Case | Description |
|----------|-------------|
| [sensor-pipeline](./host-trait/sensor-pipeline/) | Two WASM apps sharing sensor data via VFS |
| [http-cache](./host-trait/http-cache/) | HTTP server with shared VFS cache across WASM handlers |
| [concurrent-append](./host-trait/concurrent-append/) | Concurrent append test with native multithreading |

## RPC Server

| Use Case | Description |
|----------|-------------|
| [ci-cache](./rpc-server/ci-cache/) | Parallel CI jobs sharing dependency cache |
| [s3-sync-logging](./rpc-server/s3-sync-logging/) | Replicated log writers with S3 sync |
| [concurrent-append](./rpc-server/concurrent-append/) | Concurrent append test via TCP |

See each subdirectory's README for build, run, and expected output details.
