# Runtime Dynamic Linking (Recommended Approach)

This directory demonstrates **runtime dynamic linking** with WebAssembly Component Model - the **recommended approach** for maximum modularity and flexibility.

## Why This is Recommended

✓ **Maximum modularity**: Components can be updated independently
✓ **Flexible deployment**: Swap VFS implementations at runtime
✓ **Minimal overhead**: < 0.1% size increase, ~14ms composition time
✓ **Better testing**: Components can be tested in isolation
✓ **Future-proof**: Enables plugin systems and multi-tenant scenarios

## Overview

The Component Model supports two linking approaches:

1. **Dynamic Linking** (this approach - **recommended**):
   - Components are loaded **separately** at runtime
   - Linked dynamically using `wac plug` or Wasmtime's Linker API
   - Allows swapping implementations without rebuilding
   - **This is the recommended approach for new projects**

2. **Static Composition** (alternative - use `make demo-static`):
   - Components are linked at **build time** using `wac plug`
   - Produces a single `.composed.wasm` file
   - Simple deployment: one file to distribute
   - Use when single-file deployment is critical

## What This Demo Shows

This program demonstrates three key aspects:

### Part 1: Separate Component Loading
- Loads VFS Adapter component (4.24 MB)
- Loads Application component (2.46 MB)
- Shows components can be distributed and loaded independently

### Part 2: Runtime Composition
- Uses `wac plug` to compose components at runtime
- Measures composition time (~13ms)
- Executes the composed application successfully

### Part 3: Comparison
- Compares static vs dynamic composition
- Shows file sizes and overhead
- Analyzes trade-offs

## How to Build and Run

### Prerequisites

- Rust toolchain
- `wasmtime` (for component loading): `cargo install wasmtime-cli`
- `wac` (for runtime composition): `cargo install wac-cli`
- `wasm32-wasip2` target: `rustup target add wasm32-wasip2`

### Quick Start (Recommended)

From the repository root or examples directory:

```bash
make demo
# or
make demo-dynamic
```

**Works from anywhere:**
```bash
# From repository root:
cd /path/to/halycon
make demo

# From examples directory:
cd /path/to/halycon/examples
make demo
```

This automatically:
1. Builds the VFS adapter component
2. Builds the component-rust application
3. Builds the runtime linker host program
4. Runs the dynamic linking demo with full output

### Manual Build and Run

If you want to build step by step:

```bash
# From repository root:
make build-vfs-adapter       # Build VFS adapter
make build-component-rust    # Build application
make build-runtime-linker    # Build runtime linker host

# Run the demo
cd examples/runtime-linker
cargo run --release
```

## Output Explanation

The demo produces three sections of output:

```
Part 1: Loading Components Separately (Dynamic)
  • VFS Adapter: 4.24 MB (0.024s)
  • Application:  2.46 MB (0.024s)
  • Total:        6.69 MB
```
Components are loaded independently, demonstrating modularity.

```
Part 2: Runtime Linking with 'wac plug'
  ✓ Composed in 13.57ms
  ✓ Size: 6.70 MB
  ✓ Executed in 83.72ms
```
Runtime composition takes ~14ms, producing nearly the same size as static composition.

```
Part 3: Comparison
  Static Composition (build time):  6.70 MB
  Dynamic Composition (runtime):     6.70 MB
  Runtime overhead: +10 B (0.0%)
```
The overhead of runtime composition is negligible (< 0.1%).

## Key Insights

### Static Composition (Current Project Approach)

**Advantages:**
- ✓ Single file distribution
- ✓ Simpler deployment
- ✓ No runtime overhead
- ✓ Errors caught at build time

**Disadvantages:**
- ✗ Must rebuild for component updates
- ✗ Tightly coupled at build time
- ✗ Cannot swap implementations at runtime

### Dynamic Linking (Demonstrated Here)

**Advantages:**
- ✓ Independent component updates
- ✓ Swap VFS implementations at runtime
- ✓ Better modularity
- ✓ Separate distribution of components
- ✓ Multi-tenant: different instances use different implementations

**Disadvantages:**
- ✗ Multiple files to manage
- ✗ Runtime composition overhead (~14ms)
- ✗ More complex deployment
- ✗ Linking errors surface at runtime

## When to Use Each Approach

### Use Dynamic Linking (Recommended) When:
- **Building any new project** (this is the recommended default)
- You need to swap implementations at runtime
- Components update independently
- Building a plugin system
- Multi-tenant scenarios
- You need maximum modularity
- **Runtime overhead is acceptable** (~14ms is negligible for most applications)

### Use Static Composition When:
- Single-file distribution is **critical** (e.g., embedded systems, edge computing)
- Components are permanently tightly coupled
- Every millisecond counts (high-frequency trading, real-time systems)
- Deployment environment doesn't support multiple files
- You want the absolute simplest deployment model

## Technical Details

### Component Files

This demo uses:
- **VFS Adapter**: `../../target/wasm32-wasip2/debug/vfs_adapter.wasm`
  - Exports `wasi:filesystem/types@0.2.6` and other WASI interfaces
  - Provides in-memory filesystem implementation

- **Application**: `../component-rust/target/wasm32-wasip2/debug/component-rust.wasm`
  - Imports `wasi:filesystem/types@0.2.6` and other WASI interfaces
  - Uses standard Rust `std::fs` API

### Runtime Composition Process

1. **Load components separately** using `wasmtime::component::Component::from_file()`
2. **Compose using `wac plug`**: Links imports to exports
3. **Execute composed component** using `wasmtime run`

### Alternative: Wasmtime Linker API

This demo uses `wac plug` for composition. An alternative is Wasmtime's `Linker` API:

```rust
let mut linker = Linker::new(&engine);
let vfs_instance = linker.instantiate(&mut store, &vfs_adapter)?;
// Register VFS exports in linker...
let app_instance = linker.instantiate(&mut store, &app_component)?;
```

However, this is more complex and requires manual interface wiring.

## Performance Considerations

Based on measurements:
- **Component loading**: ~24ms per component
- **Runtime composition**: ~14ms
- **Execution**: ~84ms
- **Total overhead**: ~48ms vs static composition

For most applications, this overhead is negligible compared to actual work.

## Conclusion

This demo proves that **runtime dynamic linking is the recommended approach** for WebAssembly Component Model:
- ✓ Components can be distributed and loaded separately
- ✓ Runtime composition adds minimal overhead (< 0.1% size, ~14ms time)
- ✓ Provides significant flexibility and modularity benefits
- ✓ Enables independent updates and plugin systems
- ✓ Better suited for modern, modular application architectures

**This is now the recommended default approach** for:
- New projects using WebAssembly Component Model
- Applications requiring modularity and flexibility
- Systems where ~14ms overhead is negligible (most applications)
- Multi-tenant or plugin-based architectures

**Static composition remains available** as an alternative for:
- Situations where single-file deployment is critical
- Extremely latency-sensitive applications
- Embedded systems with strict deployment constraints

**Start with dynamic linking** - it provides the best foundation for scalable, maintainable WebAssembly applications.
