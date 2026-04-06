# Sensor Data Pipeline

Two WASM applications sharing an in-memory VFS via host-trait (`vfs-host`):
1. `sensor-ingest` writes simulated sensor data to `/data/sensor.log`
2. `sensor-process` reads the log and performs statistical analysis

**Deployment method**: [Host Trait](../../examples/host-trait/) (`vfs-host`)

## Build

```bash
# From repository root:
cargo build -p sensor-ingest --target wasm32-wasip2
cargo build -p sensor-process --target wasm32-wasip2
cargo build -p sensor-pipeline-runner
```

## Run

```bash
# From repository root:
cargo run -p sensor-pipeline-runner
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
