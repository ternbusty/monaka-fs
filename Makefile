.PHONY: all build build-release clean test run-example help examples build-c-example build-rust-example run-c-example run-rust-example test-c-example test-rust-example release release-c-example release-rust-example
.PHONY: demo demo-dynamic demo-static build-vfs-adapter build-component-rust build-component-c build-components compose-static-rust compose-static-c run-static-rust run-static-c build-runtime-linker

# Default target - show demo (recommended: dynamic linking)
all: demo

# Show the recommended demo (dynamic linking)
demo: demo-dynamic

# Build libraries (fs-core, fs-wasm)
build:
	@echo "Building libraries (fs-core, fs-wasm)..."
	@cargo build -p fs-core -p fs-wasm
	@echo "Libraries built successfully"

# Build libraries in release mode
build-release:
	@echo "Building libraries (release mode)..."
	@cargo build -p fs-core -p fs-wasm --release
	@echo "Release libraries built successfully"

# Build both examples
examples: build-c-example build-rust-example

# Build the C example WASM module (using Makefile, no cargo)
build-c-example:
	@echo "Building C integration example (Makefile)..."
	@$(MAKE) -C examples/c

# Build the Rust example WASM module
build-rust-example:
	@echo "Building Rust example..."
	@cargo build -p rust-example --target wasm32-wasip1
	@echo "Build complete: target/wasm32-wasip1/debug/rust-example.wasm"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean
	@$(MAKE) -C examples/c clean
	@echo "Clean complete"

# Test/Run C example
test-c-example:
	@$(MAKE) -C examples/c run

run-c-example: test-c-example

# Test/Run Rust example
test-rust-example: build-rust-example
	@echo "Running Rust example..."
	@wasmtime run target/wasm32-wasip1/debug/rust-example.wasm

run-rust-example: test-rust-example

# Run both examples
test: test-c-example test-rust-example
run-example: run-c-example run-rust-example

# Release builds
release: release-c-example release-rust-example

release-c-example:
	@echo "Building C example (release)..."
	@$(MAKE) -C examples/c BUILD_MODE=release
	@echo "Release build complete: target/wasm32-wasip1/release/c_example.wasm"
	@ls -lh target/wasm32-wasip1/release/c_example.wasm

release-rust-example:
	@echo "Building Rust example (release)..."
	@cargo build -p rust-example --target wasm32-wasip1 --release
	@echo "Release build complete: target/wasm32-wasip1/release/rust-example.wasm"
	@ls -lh target/wasm32-wasip1/release/rust-example.wasm

# Check build prerequisites
check-prereqs:
	@echo "Checking build prerequisites..."
	@echo -n "WASI-libc: "
	@if brew list wasi-libc >/dev/null 2>&1; then echo "installed"; else echo "missing"; fi
	@echo -n "LLVM: "
	@if brew list llvm >/dev/null 2>&1; then echo "installed"; else echo "missing"; fi
	@echo -n "wasm32-wasip1 target: "
	@if rustup target list --installed | grep -q wasm32-wasip1; then echo "installed"; else echo "missing"; fi
	@echo -n "wasmtime: "
	@if command -v wasmtime >/dev/null 2>&1; then echo "available"; else echo "missing"; fi

# Install prerequisites
install-prereqs:
	@echo "Installing prerequisites..."
	@brew install wasi-libc llvm
	@rustup target add wasm32-wasip1
	@echo "Prerequisites installed"

# Show file information
info:
	@echo "Ephemeral Filesystem Examples"
	@echo "=============================="
	@echo ""
	@if [ -f target/wasm32-wasip1/debug/c_example.wasm ]; then \
		echo "C Example:"; \
		echo "  File: target/wasm32-wasip1/debug/c_example.wasm"; \
		echo "  Size: $$(ls -lh target/wasm32-wasip1/debug/c_example.wasm | awk '{print $$5}')"; \
		echo "  Run:  wasmtime run target/wasm32-wasip1/debug/c_example.wasm"; \
		echo "        OR: make run-c"; \
		echo ""; \
	fi
	@if [ -f target/wasm32-wasip1/debug/rust-example.wasm ]; then \
		echo "Rust Example:"; \
		echo "  File: target/wasm32-wasip1/debug/rust-example.wasm"; \
		echo "  Size: $$(ls -lh target/wasm32-wasip1/debug/rust-example.wasm | awk '{print $$5}')"; \
		echo "  Run:  wasmtime run target/wasm32-wasip1/debug/rust-example.wasm"; \
		echo "        OR: make run-rust"; \
		echo ""; \
	fi

# Benchmark/performance test
benchmark: examples
	@echo "Running performance benchmarks..."
	@echo ""
	@echo "C Example:"
	@time wasmtime run target/wasm32-wasip1/debug/c_example.wasm >/dev/null
	@echo ""
	@echo "Rust Example:"
	@time wasmtime run target/wasm32-wasip1/debug/rust-example.wasm >/dev/null

#
# Component Model - Dynamic Linking
#

# Build VFS adapter component
build-vfs-adapter:
	@echo "Building VFS adapter component..."
	@cargo build -p vfs-adapter --target wasm32-wasip2
	@echo "Built: target/wasm32-wasip2/debug/vfs_adapter.wasm"

# Build component-rust
build-component-rust:
	@echo "Building component-rust..."
	@cd examples/component-rust && cargo build --target wasm32-wasip2
	@echo "Built: examples/component-rust/target/wasm32-wasip2/debug/component-rust.wasm"

# Build component-c
build-component-c:
	@echo "Building component-c..."
	@cd examples/component-c && cargo build --target wasm32-wasip2
	@echo "Built: examples/component-c/target/wasm32-wasip2/debug/component-c.wasm"

# Build all components
build-components: build-vfs-adapter build-component-rust build-component-c

# Build runtime linker host program
build-runtime-linker:
	@echo "Building runtime linker host program..."
	@cd examples/runtime-linker && cargo build --release
	@echo "Built: examples/runtime-linker/target/release/runtime-linker"

# Run dynamic linking demo (RECOMMENDED)
demo-dynamic: build-components build-runtime-linker
	@echo ""
	@echo "╔═══════════════════════════════════════════════════╗"
	@echo "║   Runtime Dynamic Linking Demo (Recommended)     ║"
	@echo "╚═══════════════════════════════════════════════════╝"
	@echo ""
	@cd examples/runtime-linker && cargo run --release

#
# Component Model - Static Composition (Alternative)
#

# Compose component-rust with VFS adapter at build time
compose-static-rust: build-vfs-adapter build-component-rust
	@echo "Composing component-rust with VFS adapter (build-time)..."
	@cd examples && wac plug \
		--plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
		component-rust/target/wasm32-wasip2/debug/component-rust.wasm \
		-o component-rust.composed.wasm
	@echo "Composed: examples/component-rust.composed.wasm"

# Compose component-c with VFS adapter at build time
compose-static-c: build-vfs-adapter build-component-c
	@echo "Composing component-c with VFS adapter (build-time)..."
	@cd examples && wac plug \
		--plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
		component-c/target/wasm32-wasip2/debug/component-c.wasm \
		-o component-c.composed.wasm
	@echo "Composed: examples/component-c.composed.wasm"

# Run statically composed component-rust
run-static-rust: compose-static-rust
	@echo "Running statically composed component-rust..."
	@wasmtime run examples/component-rust.composed.wasm

# Run statically composed component-c
run-static-c: compose-static-c
	@echo "Running statically composed component-c..."
	@wasmtime run examples/component-c.composed.wasm

# Run both static composition demos
demo-static: run-static-rust run-static-c

# Help target
help:
	@echo "halycon - WebAssembly Component Model Filesystem"
	@echo "================================================="
	@echo ""
	@echo "╔═════════════════════════════════════════════════════════════╗"
	@echo "║  RECOMMENDED: make demo  (Runtime Dynamic Linking)          ║"
	@echo "╚═════════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "Quick Start:"
	@echo "  make demo              - Run dynamic linking demo (RECOMMENDED)"
	@echo "  make demo-dynamic      - Same as 'make demo'"
	@echo "  make demo-static       - Run static composition demos"
	@echo ""
	@echo "Component Model - Dynamic Linking (Recommended):"
	@echo "  make build-vfs-adapter    - Build VFS adapter component"
	@echo "  make build-component-rust - Build component-rust"
	@echo "  make build-component-c    - Build component-c"
	@echo "  make build-components     - Build all components"
	@echo "  make build-runtime-linker - Build runtime linker host"
	@echo "  make demo-dynamic         - Run dynamic linking demo"
	@echo ""
	@echo "Component Model - Static Composition (Alternative):"
	@echo "  make compose-static-rust - Compose component-rust (build-time)"
	@echo "  make compose-static-c    - Compose component-c (build-time)"
	@echo "  make run-static-rust     - Run composed component-rust"
	@echo "  make run-static-c        - Run composed component-c"
	@echo "  make demo-static         - Run both static demos"
	@echo ""
	@echo "Legacy Examples (Direct Library Usage):"
	@echo "  make build-c-example      - Build C integration example"
	@echo "  make build-rust-example   - Build Rust example"
	@echo "  make run-c-example        - Run C integration example"
	@echo "  make run-rust-example     - Run Rust example"
	@echo "  make examples             - Build all legacy examples"
	@echo "  make run-example          - Run all legacy examples"
	@echo ""
	@echo "Library Build Commands:"
	@echo "  make build                - Build libraries (fs-core, fs-wasm)"
	@echo "  make build-release        - Build libraries (release mode)"
	@echo "  make clean                - Clean all build artifacts"
	@echo ""
	@echo "Utility Commands:"
	@echo "  make check-prereqs        - Check build prerequisites"
	@echo "  make install-prereqs      - Install missing prerequisites"
	@echo "  make info                 - Show module information"
	@echo "  make benchmark            - Run performance test"
	@echo "  make help                 - Show this help message"
	@echo ""
	@echo "Documentation:"
	@echo "  README.md                        - Project overview"
	@echo "  CLAUDE.md                        - Development guide"
	@echo "  examples/README.md               - All examples overview"
	@echo "  examples/runtime-linker/README.md- Dynamic linking details"
