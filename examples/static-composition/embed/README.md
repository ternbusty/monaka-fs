# Static Composition: File Embedding Example

Demonstrates embedding local files into a `vfs-adapter` WASM binary using `halycon-pack`, then composing with an application via `wac plug`.

This follows the same pattern as [wasi-virt](https://github.com/bytecodealliance/wasi-virt): embed files into the adapter first, then compose with the app.

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install wac-cli wasmtime-cli
cargo install --path crates/tools/halycon-pack
```

## Build

```bash
# From repository root:

# Build the VFS adapter
cargo build --release -p vfs-adapter --target wasm32-wasip2

# Build the example app
cargo build --release -p demo-embed-read --target wasm32-wasip2
```

## Embed & Compose

```bash
# From repository root:

# Step 1: Embed files into the adapter
halycon-pack embed \
  --mount "/data=examples/static-composition/embed/testdata" \
  -o /tmp/vfs-adapter-packed.wasm \
  target/wasm32-wasip2/release/vfs_adapter.wasm

# Step 2: Compose with the app
wac plug \
  --plug /tmp/vfs-adapter-packed.wasm \
  target/wasm32-wasip2/release/demo-embed-read.wasm \
  -o /tmp/embed-example.wasm
```

## Run

```bash
wasmtime run /tmp/embed-example.wasm
```

## Expected Output

```
=== Embedded File Read Test ===

Listing /data:
  /data/hello.txt
  /data/world.txt

Reading /data/hello.txt:
  "hello from embedded file"

Reading /data/world.txt:
  "another file"

=== Done ===
```
