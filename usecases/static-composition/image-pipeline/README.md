# Image Pipeline Demo

File-based image processing pipeline using static composition (`vfs-adapter` + `wac plug`). The app performs resize and format conversion using intermediate files in the in-memory VFS.

**Deployment method**: [Static Composition](../../examples/static-composition/) (`vfs-adapter`)

```
/input/photo.raw  --[resize]-->  /work/resized.dat  --[convert]-->  /output/photo.png
```

## Build

```bash
# From repository root:
cargo build -p image-processor --target wasm32-wasip2
cargo build -p vfs-adapter --target wasm32-wasip2
wac plug \
  --plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
  target/wasm32-wasip2/debug/image-processor.wasm \
  -o target/wasm32-wasip2/debug/image-processor-composed.wasm
```

## Run

```bash
# From repository root:
wasmtime run target/wasm32-wasip2/debug/image-processor-composed.wasm
```

## Expected Output

```
=== Image Processing Pipeline ===

Created input: /input/photo.raw (1030 bytes)

--- Step 1: Resize ---
[RESIZE] Input: 1030 bytes
[RESIZE] Downscaled 1024 -> 256 pixels
[RESIZE] Output: /work/resized.dat (268 bytes)

--- Step 2: Convert Format ---
[CONVERT] Input: 268 bytes
[CONVERT] Pixel data: 256 bytes
[CONVERT] Output: /output/photo.png (272 bytes)

=== Pipeline Complete ===
Output: /output/photo.png (272 bytes)
PNG header verified!
```
