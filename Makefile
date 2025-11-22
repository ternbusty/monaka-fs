.PHONY: all build build-release clean test run-example help examples build-c-example build-rust-example run-c-example run-rust-example test-c-example test-rust-example release release-c-example release-rust-example

# Default target - build libraries only
all: build

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

# Help target
help:
	@echo "ephemeral-fs - Make Commands"
	@echo "============================"
	@echo ""
	@echo "Build Commands:"
	@echo " make build                - Build libraries only (fs-core, fs-wasm)"
	@echo " make build-release        - Build libraries in release mode"
	@echo " make examples             - Build all examples (C + Rust)"
	@echo " make build-c-example      - Build C integration example"
	@echo " make build-rust-example   - Build Rust example"
	@echo " make release              - Build all examples (optimized)"
	@echo " make release-c-example    - Build C example (optimized)"
	@echo " make release-rust-example - Build Rust example (optimized)"
	@echo " make clean                - Clean all build artifacts"
	@echo ""
	@echo "Run Commands:"
	@echo " make run-example          - Run all examples"
	@echo " make run-c-example        - Run C integration example"
	@echo " make run-rust-example     - Run Rust example"
	@echo ""
	@echo "Utility Commands:"
	@echo " make check-prereqs  - Check build prerequisites"
	@echo " make install-prereqs- Install missing prerequisites"
	@echo " make info           - Show module information"
	@echo " make benchmark      - Run performance test"
	@echo " make help           - Show this help message"
	@echo ""
	@echo "Direct Commands:"
	@echo " cargo build                                          # Libraries only"
	@echo " make -C examples/c                                   # C example (Makefile)"
	@echo " cargo build -p rust-example --target wasm32-wasip1  # Rust example"
	@echo ""
	@echo "Documentation:"
	@echo " README.md              - Project overview"
	@echo " CLAUDE.md              - Development guide"
	@echo " examples/c/README.md   - C integration details"
	@echo " examples/rust/README.md- Rust example guide"
