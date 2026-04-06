# halycon-pack

CLI tool to embed files into vfs-adapter WASM binaries.

## Install

```bash
cargo install --path crates/tools/halycon-pack
```

## Usage

```bash
# Embed local files into the adapter
halycon-pack embed --mount /data=./local-dir -o vfs-adapter-packed.wasm vfs-adapter.wasm

# Compose with your app
wac plug --plug vfs-adapter-packed.wasm app.wasm -o composed.wasm

# Run
wasmtime run composed.wasm
```

Multiple mounts can be specified:

```bash
halycon-pack embed \
  --mount /config=./config \
  --mount /data=./data \
  -o vfs-adapter-packed.wasm \
  vfs-adapter.wasm
```

## Example

See [examples/static-composition/embed](../../examples/static-composition/embed/) for a working example.
