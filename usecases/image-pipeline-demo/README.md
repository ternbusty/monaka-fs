# Image Pipeline Demo

Demonstrates `wac plug` component composition with VFS for file-based data pipelines.

## Running the Demo

```bash
make run-usecase-image-pipeline
```

## How It Works

### Architecture

```
wasmtime run image-processor-composed.wasm
         │
         └─> [image-processor.wasm + vfs_adapter.wasm]
                    │
                    ├── Step 1: resize
                    │   /input/photo.raw → /work/resized.dat
                    │
                    └── Step 2: convert
                        /work/resized.dat → /output/photo.png
```

### Pipeline

```
/input/photo.raw  ──[resize]──>  /work/resized.dat  ──[convert]──>  /output/photo.png
     (1030 bytes)        (downscale 4x)        (268 bytes)      (add PNG header)     (~272 bytes)
```

### Component Composition

The WASM app is composed with `vfs_adapter.wasm` using `wac plug`:

```bash
wac plug \
  --plug vfs_adapter.wasm \
  image_processor.wasm \
  -o image-processor-composed.wasm
```

This allows the app to use `std::fs` APIs that transparently operate on the in-memory VFS.

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
