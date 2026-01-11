# Benchmark 04: RPC VFS with/without S3 Sync

Compares RPC VFS performance with and without S3 persistence.

## Running

### Mac

```bash
cd benchmarks/04-s3fs-vs-s3sync
./run-bench.sh
```

### Linux VM (with s3fs-fuse comparison)

By default, s3 sync works asynchronously.

```bash
./run-bench-vm.sh
```

You can also try realtime mode.

```bash
./run-bench-vm-realtime.sh
```
