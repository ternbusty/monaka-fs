# monaka

CLI tool for Monaka VFS. Embeds files, composes WASM components, and extracts bundled binaries.

All required `.wasm` adapters and servers are bundled inside the CLI binary.

## Build

```bash
make build-cli
```

## Commands

### embed

Embed local files into the bundled vfs-adapter:

```bash
monaka embed --mount /data=./local-dir -o vfs-adapter-packed.wasm
monaka embed --mount /data=./local-dir --s3-sync -o vfs-adapter-s3-packed.wasm
```

### compose

Compose an app with a bundled adapter:

```bash
# Static composition (vfs-adapter)
monaka compose app.wasm -o composed.wasm

# With S3 sync
monaka compose --s3-sync app.wasm -o composed.wasm

# With RPC adapter
monaka compose --rpc app.wasm -o composed.wasm

# Embed files and compose in one step
monaka compose --mount /data=./local-dir app.wasm -o composed.wasm
```

### extract

Extract a bundled WASM binary to a file:

```bash
monaka extract adapter -o vfs-adapter.wasm
monaka extract adapter --s3-sync -o vfs-adapter-s3.wasm
monaka extract rpc-adapter -o rpc-adapter.wasm
monaka extract server -o vfs-rpc-server.wasm
monaka extract server --s3-sync -o vfs-rpc-server-s3.wasm
```

## Building from source

```bash
# From repository root — builds all WASM components then the CLI
make build-cli
```

## Example

See [examples/static-composition/embed](../../examples/static-composition/embed/) for a working example.
