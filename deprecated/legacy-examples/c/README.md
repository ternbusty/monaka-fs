# C Example (Legacy/Deprecated)

This directory demonstrates C code using the Rust-based ephemeral filesystem (fs-wasm).

Note: This is a legacy example using the deprecated fs-wasm library with wasm32-wasip1 target. For the current approach, see the examples in `/examples/` which use wasm32-wasip2.

## How to Build

```bash
# From repository root
make build-legacy-c           # Debug build
make build-legacy-c-release   # Release build
```

## How to Run

```bash
# From repository root
make run-legacy-c

# Or run directly
wasmtime run ./target/wasm32-wasip1/debug/c_example.wasm
```

## Build Process

1. Build fs-core as .rlib (Rust library)
2. Build fs-wasm as .rlib (depends on fs-core)
3. Compile main.c to main.o (WASM object)
4. Link stub.rs + C object + libraries -> c_example.wasm
