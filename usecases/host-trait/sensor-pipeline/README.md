# Sensor Data Pipeline

Two WASM applications sharing an in-memory VFS via host-trait (`vfs-host`):
1. `sensor-ingest` writes simulated sensor data to `/data/sensor.log`
2. `sensor-process` reads the log and performs statistical analysis

**Deployment method**: Host Trait (`vfs-host` crate — `cargo add vfs-host`)

> This use case demonstrates the Host Trait method, where a native Rust program hosts WASM instances sharing a single VFS. The `monaka` CLI is not used here; instead, the host program (`sensor-pipeline-runner`) links against `vfs-host` directly.

## Build

```bash
# From repository root:

# Build the WASM apps (standalone packages)
cd usecases/host-trait/sensor-pipeline/sensor-ingest && cargo build --target wasm32-wasip2 && cd ../../../..
cd usecases/host-trait/sensor-pipeline/sensor-process && cargo build --target wasm32-wasip2 && cd ../../../..

# Build the host binary (standalone package)
cd usecases/host-trait/sensor-pipeline/runtime-linker
cargo build
```

## Run

```bash
# From usecases/host-trait/sensor-pipeline/runtime-linker/:
cargo run
```

## Expected Output

```
=== VFS Sharing Demo: Sensor Data Pipeline ===

Demonstrating data pipeline between two WASM applications:
  1. sensor-ingest: Collects sensor data, writes to /data/sensor.log
  2. sensor-process: Reads log, performs statistical analysis

--- Running sensor-ingest ---
[INGEST] Writing sensor data to /data/sensor.log
[INGEST] Wrote 5 sensor readings

--- Running sensor-process ---
[PROCESS] Reading sensor data from /data/sensor.log
[PROCESS] Parsed 5 readings
[PROCESS] Temperature - avg: ... min: ... max: ...
[PROCESS] Humidity    - avg: ... min: ... max: ...

=== Demo Complete ===
```
