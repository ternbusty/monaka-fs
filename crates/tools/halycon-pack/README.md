# halycon-pack

CLI tool to pack files into vfs-adapter WASM binaries.

## Usage

```bash
# Generate a snapshot JSON from local files
halycon-pack snapshot --mount /data=./local-dir -o snapshot.json

# Build vfs-adapter with the snapshot embedded
HALYCON_SNAPSHOT=snapshot.json cargo build -p vfs-adapter --target wasm32-wasip2 --release

# Compose with your app
wac plug --plug target/wasm32-wasip2/release/vfs_adapter.wasm app.wasm -o composed.wasm
```

## Options

```
halycon-pack snapshot [OPTIONS]

Options:
  -o, --output <FILE>     Output snapshot file (JSON format) [required]
  -m, --mount <MOUNT>     Mount a local directory into the virtual filesystem [required]
                          Format: /virtual-path=./local-path
```

Multiple mounts can be specified:

```bash
halycon-pack snapshot \
  --mount /config=./config \
  --mount /data=./data \
  -o snapshot.json
```

## Demo

### 1. Create test data

```bash
mkdir -p .tmp/test-data/config
echo "Hello from embedded file!" > .tmp/test-data/hello.txt
echo '{"version": "1.0", "name": "test"}' > .tmp/test-data/config/settings.json
```

### 2. Generate snapshot

```bash
cargo run -p halycon-pack -- snapshot \
  --mount /data=.tmp/test-data \
  -o .tmp/snapshot.json
```

### 3. Build vfs-adapter with embedded files

```bash
HALYCON_SNAPSHOT=.tmp/snapshot.json \
  cargo build -p vfs-adapter --target wasm32-wasip2 --release
```

### 4. Compose with a test app

```bash
# Build test app (e.g., demo-fs-operations)
cargo build -p static-rust --target wasm32-wasip2 --release

# Compose
wac plug \
  --plug target/wasm32-wasip2/release/vfs_adapter.wasm \
  target/wasm32-wasip2/release/static_rust.wasm \
  -o .tmp/composed.wasm
```

### 5. Run

```bash
wasmtime .tmp/composed.wasm
```

The app can now access `/data/hello.txt` and `/data/config/settings.json` using standard `std::fs` APIs.
