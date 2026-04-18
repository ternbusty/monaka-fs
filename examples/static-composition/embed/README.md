# Static Composition: File Embedding Example

Demonstrates embedding local files into `vfs-adapter` and composing with an application.

## Using `monaka` CLI

```bash
# Build the app
cargo build --release -p demo-embed-read --target wasm32-wasip2

# Embed files and compose in one step
make build-cli
target/release/monaka compose \
  --mount "/data=examples/static-composition/embed/testdata" \
  target/wasm32-wasip2/release/demo-embed-read.wasm \
  -o /tmp/embed-example.wasm

# Run
wasmtime run /tmp/embed-example.wasm
```

## Expected Output

```
/data/dir1 (dir)
  /data/dir1/test1.txt:
    "Hello from test1"
/data/test2.txt:
  "Hello from test1"
```

## Manual Setup (without `monaka` CLI)

### Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
cargo install --path crates/tools/monaka-cli
```

### Build & Compose

```bash
# From repository root:

# Build vfs-adapter and the app
cargo build --release -p vfs-adapter --target wasm32-wasip2
cargo build --release -p demo-embed-read --target wasm32-wasip2

# Embed files into the adapter
monaka embed \
  --mount "/data=examples/static-composition/embed/testdata" \
  -o /tmp/vfs-adapter-packed.wasm

# Compose with the app
wac plug \
  --plug /tmp/vfs-adapter-packed.wasm \
  target/wasm32-wasip2/release/demo-embed-read.wasm \
  -o /tmp/embed-example.wasm

# Run
wasmtime run /tmp/embed-example.wasm
```
