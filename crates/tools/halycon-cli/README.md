# halycon

CLI tool for Halycon VFS. Embeds files, composes WASM components, and extracts bundled binaries.

All required `.wasm` adapters and servers are bundled inside the CLI binary.

## Build

```bash
make build-cli
```

## Commands

### embed

Embed local files into the bundled vfs-adapter:

```bash
halycon embed --mount /data=./local-dir -o vfs-adapter-packed.wasm
halycon embed --mount /data=./local-dir --s3-sync -o vfs-adapter-s3-packed.wasm
```

### compose

Compose an app with a bundled adapter:

```bash
# Static composition (vfs-adapter)
halycon compose app.wasm -o composed.wasm

# With S3 sync
halycon compose --s3-sync app.wasm -o composed.wasm

# With RPC adapter
halycon compose --rpc app.wasm -o composed.wasm

# Embed files and compose in one step
halycon compose --mount /data=./local-dir app.wasm -o composed.wasm
```

### extract

Extract a bundled WASM binary to a file:

```bash
halycon extract adapter -o vfs-adapter.wasm
halycon extract adapter --s3-sync -o vfs-adapter-s3.wasm
halycon extract rpc-adapter -o rpc-adapter.wasm
halycon extract server -o vfs-rpc-server.wasm
halycon extract server --s3-sync -o vfs-rpc-server-s3.wasm
```

## Building from source

```bash
# From repository root — builds all WASM components then the CLI
make build-cli
```

## Example

See [examples/static-composition/embed](../../examples/static-composition/embed/) for a working example.
