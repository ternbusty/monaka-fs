# CI Cache Demo

Demonstrates multiple CI jobs sharing dependency cache via VFS RPC server.
Each job acquires per-library locks to ensure safe concurrent access.

## Running the Demo

```bash
make run-usecase-ci-cache
```

## How It Works

### Architecture

```
[VFS RPC Server]  <-- Shared filesystem (TCP:9000)
      ^
      |
+-----+-----+---------+
|     |     |         |
[Job1]  [Job2]  [Job3]   <-- Parallel WASM processes
  |       |       |
  +-------+-------+
     /cache/ shared
```

### Cache Structure

```
/cache/
  serde-1.0.0.cache      # Library cache file
  serde-1.0.0.lock/      # Lock directory (exists = locked)
  tokio-1.0.0.cache
  anyhow-1.0.0.cache
```

### Job Dependencies

| Job | Dependencies |
|-----|--------------|
| Job1 | serde-1.0.0, tokio-1.0.0 |
| Job2 | serde-1.0.0, anyhow-1.0.0 |
| Job3 | tokio-1.0.0, anyhow-1.0.0 |

### Locking Protocol

For each dependency:
1. **Acquire lock**: `mkdir /cache/{lib}.lock` (atomic, fails if exists)
2. **Check cache**: Read `/cache/{lib}.cache` if exists
3. **On miss**: Simulate download, write cache file
4. **Release lock**: `rmdir /cache/{lib}.lock`

## Expected Output

```
[Job1] Starting with deps: serde-1.0.0, tokio-1.0.0
[Job2] Starting with deps: serde-1.0.0, anyhow-1.0.0
[Job3] Starting with deps: tokio-1.0.0, anyhow-1.0.0

[Job1] serde-1.0.0: acquiring lock...
[Job2] serde-1.0.0: acquiring lock...
[Job1] serde-1.0.0: lock acquired
[Job1] serde-1.0.0: MISS - downloading...
[Job3] tokio-1.0.0: acquiring lock...
[Job3] tokio-1.0.0: lock acquired
[Job3] tokio-1.0.0: MISS - downloading...
[Job1] serde-1.0.0: cached (52 bytes)
[Job1] serde-1.0.0: lock released
[Job2] serde-1.0.0: lock acquired
[Job2] serde-1.0.0: HIT (52 bytes)
[Job2] serde-1.0.0: lock released
...
[Job1] Done
[Job2] Done
[Job3] Done
```