# halycon Makefile
# Naming pattern: action-target-mode (e.g., build-component-model-rust-release)

.PHONY: build build-release build-all build-native build-wasm build-wasm-release clean help
.PHONY: demo-component-model-dynamic demo-component-model-static
.PHONY: build-component-model-adapter build-component-model-rust build-component-model-c build-component-model-all build-component-model-linker
.PHONY: compose-component-model-rust compose-component-model-c
.PHONY: run-component-model-static-rust run-component-model-static-c
.PHONY: build-legacy-c build-legacy-rust build-legacy-all
.PHONY: build-legacy-c-release build-legacy-rust-release build-legacy-all-release
.PHONY: run-legacy-c run-legacy-rust run-legacy-all
.PHONY: run-legacy-c-release run-legacy-rust-release run-legacy-all-release
.PHONY: build-rpc-server build-rpc-demos build-rpc-all
.PHONY: build-rpc-adapter compose-rpc-demos build-rpc-composed
.PHONY: start-rpc-server stop-rpc-server run-rpc-demo-writer run-rpc-demo-reader run-rpc-demo-std-fs
.PHONY: run-rpc-composed-writer run-rpc-composed-reader run-rpc-composed-test
.PHONY: build-usecase-sensor-pipeline run-usecase-sensor-pipeline
.PHONY: build-usecase-s3-sync-logging compose-usecase-s3-sync-logging run-usecase-s3-sync-logging
.PHONY: build-usecase-http-cache run-usecase-http-cache
.PHONY: build-usecase-ci-cache run-usecase-ci-cache
.PHONY: build-usecase-image-pipeline run-usecase-image-pipeline
.PHONY: check-prereqs install-prereqs info benchmark

# =============================================================================
# Library
# =============================================================================

# Build libraries (fs-core only)
build:
	@echo "Building libraries (fs-core)..."
	@cargo build -p fs-core
	@echo "Libraries built successfully"

# Build libraries in release mode
build-release:
	@echo "Building libraries (release mode)..."
	@cargo build -p fs-core --release
	@echo "Release libraries built successfully"

# Build all packages (native + WASM)
build-all: build-native build-wasm
	@echo "All packages built successfully"

# Build native packages only
build-native:
	@echo "Building native packages..."
	@cargo build
	@echo "Native packages built"

# Build all WASM packages
build-wasm:
	@echo "Building WASM packages..."
	@cargo build --target wasm32-wasip2 \
		-p vfs-adapter \
		-p rpc-adapter \
		-p vfs-rpc-server \
		-p demo-writer \
		-p demo-reader \
		-p demo-std-fs \
		-p direct-rpc-demo \
		-p sensor-ingest \
		-p sensor-process \
		-p logger
	@echo "WASM packages built"

# Build all WASM packages (release)
build-wasm-release:
	@echo "Building WASM packages (release)..."
	@cargo build --release --target wasm32-wasip2 \
		-p vfs-adapter \
		-p rpc-adapter \
		-p vfs-rpc-server \
		-p demo-writer \
		-p demo-reader \
		-p demo-std-fs \
		-p direct-rpc-demo \
		-p sensor-ingest \
		-p sensor-process \
		-p logger
	@echo "WASM packages built (release)"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean
	@$(MAKE) -C deprecated/legacy-examples/c clean
	@rm -f examples/*.composed.wasm
	@echo "Clean complete"

# =============================================================================
# Component Model (Build)
# =============================================================================

# Build VFS adapter component
build-component-model-adapter:
	@echo "Building VFS adapter component..."
	@cargo build -p vfs-adapter --target wasm32-wasip2
	@echo "Built: target/wasm32-wasip2/debug/vfs_adapter.wasm"

# Build component-rust
build-component-model-rust:
	@echo "Building component-rust..."
	@cd examples/component-model/static/rust && cargo build --target wasm32-wasip2
	@echo "Built: examples/component-model/static/rust/target/wasm32-wasip2/debug/component-rust.wasm"

# Build component-c
build-component-model-c:
	@echo "Building component-c..."
	@cd examples/component-model/static/c && cargo build --target wasm32-wasip2
	@echo "Built: examples/component-model/static/c/target/wasm32-wasip2/debug/component-c.wasm"

# Build all components
build-component-model-all: build-component-model-adapter build-component-model-rust build-component-model-c

# Build runtime linker host program
build-component-model-linker:
	@echo "Building runtime linker host program..."
	@cd examples/component-model/runtime-linker && cargo build --release
	@echo "Built: examples/component-model/runtime-linker/target/release/runtime-linker"

# Build sensor pipeline usecase
build-usecase-sensor-pipeline:
	@echo "Building sensor pipeline usecase..."
	@cargo build -p sensor-ingest --target wasm32-wasip2
	@cargo build -p sensor-process --target wasm32-wasip2
	@cargo build -p sensor-pipeline-runner --release
	@echo "Built: usecases/sensor-pipeline/"

# =============================================================================
# Component Model (Demo)
# =============================================================================

# Run dynamic linking demo
demo-component-model-dynamic: build-component-model-adapter build-component-model-linker build-usecase-sensor-pipeline
	@echo ""
	@echo "=============================================="
	@echo "  Component Model: Runtime Dynamic Linking"
	@echo "=============================================="
	@echo ""
	@cd examples/component-model/runtime-linker && cargo run --release

# =============================================================================
# Component Model (Static Composition)
# =============================================================================

# Compose component-rust with VFS adapter at build time
compose-component-model-rust: build-component-model-adapter build-component-model-rust
	@echo "Composing component-rust with VFS adapter (build-time)..."
	@cd examples && wac plug \
		--plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
		component-model/static/rust/target/wasm32-wasip2/debug/component-rust.wasm \
		-o component-rust.composed.wasm
	@echo "Composed: examples/component-rust.composed.wasm"

# Compose component-c with VFS adapter at build time
compose-component-model-c: build-component-model-adapter build-component-model-c
	@echo "Composing component-c with VFS adapter (build-time)..."
	@cd examples && wac plug \
		--plug ../target/wasm32-wasip2/debug/vfs_adapter.wasm \
		component-model/static/c/target/wasm32-wasip2/debug/component_c.wasm \
		-o component-c.composed.wasm
	@echo "Composed: examples/component-c.composed.wasm"

# Run statically composed component-rust
run-component-model-static-rust: compose-component-model-rust
	@echo "Running statically composed component-rust..."
	@wasmtime run examples/component-rust.composed.wasm

# Run statically composed component-c
run-component-model-static-c: compose-component-model-c
	@echo "Running statically composed component-c..."
	@wasmtime run examples/component-c.composed.wasm

# Run both static composition demos
demo-component-model-static: run-component-model-static-rust run-component-model-static-c

# =============================================================================
# RPC
# =============================================================================

# Build VFS RPC server
build-rpc-server:
	@echo "Building VFS RPC server..."
	@cargo build -p vfs-rpc-server --target wasm32-wasip2
	@echo "Built: target/wasm32-wasip2/debug/vfs_rpc_server.wasm"

# Build RPC demo applications
build-rpc-demos:
	@echo "Building RPC demo applications..."
	@cargo build -p demo-writer --target wasm32-wasip2
	@cargo build -p demo-reader --target wasm32-wasip2
	@cargo build -p demo-std-fs --target wasm32-wasip2
	@cargo build -p direct-rpc-demo --target wasm32-wasip2
	@echo "Built all RPC demos"

# Build all RPC components
build-rpc-all: build-rpc-server build-rpc-demos

# Start VFS RPC server (runs in background)
start-rpc-server: build-rpc-server
	@echo "Starting VFS RPC server on port 9000..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm &
	@echo "Server started. Use 'make stop-rpc-server' to stop."

# Stop VFS RPC server
stop-rpc-server:
	@echo "Stopping VFS RPC server..."
	@pkill -f vfs_rpc_server.wasm || true
	@echo "Server stopped."

# Run demo-writer (requires server running)
run-rpc-demo-writer: compose-rpc-demos
	@echo "Running demo-writer..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm

# Run demo-reader (requires server running)
run-rpc-demo-reader: compose-rpc-demos
	@echo "Running demo-reader..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm

# Run demo-std-fs (requires server running, uses direct RPC)
run-rpc-demo-std-fs: build-rpc-demos
	@echo "Running demo-std-fs..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/demo_std_fs.wasm

# =============================================================================
# RPC Composed (Build-time composition with wac - no native binary needed)
# =============================================================================

# Build rpc-adapter WASM component
build-rpc-adapter:
	@echo "Building rpc-adapter..."
	@cargo build -p rpc-adapter --target wasm32-wasip2
	@echo "Built: target/wasm32-wasip2/debug/rpc_adapter.wasm"

# Compose demo apps with rpc-adapter (build-time composition)
compose-rpc-demos: build-rpc-adapter build-rpc-demos
	@echo "Composing demo-writer with rpc-adapter..."
	@wac plug \
		--plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
		target/wasm32-wasip2/debug/demo-writer.wasm \
		-o target/wasm32-wasip2/debug/composed-demo-writer.wasm
	@echo "Composing demo-reader with rpc-adapter..."
	@wac plug \
		--plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
		target/wasm32-wasip2/debug/demo-reader.wasm \
		-o target/wasm32-wasip2/debug/composed-demo-reader.wasm
	@echo "Composed components created"

# Build all composed components
build-rpc-composed: build-rpc-server compose-rpc-demos
	@echo "All RPC composed components built"

# Run composed demo-writer (requires server running, no native binary needed)
run-rpc-composed-writer: compose-rpc-demos
	@echo "Running composed demo-writer..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm

# Run composed demo-reader (requires server running, no native binary needed)
run-rpc-composed-reader: compose-rpc-demos
	@echo "Running composed demo-reader..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm

# Run full composed test (starts server, runs writer, runs reader, stops server)
run-rpc-composed-test: build-rpc-composed
	@echo "=============================================="
	@echo "  RPC Composed Component Test"
	@echo "=============================================="
	@pkill -f vfs_rpc_server.wasm 2>/dev/null || true
	@sleep 1
	@echo "Starting VFS RPC server..."
	@wasmtime run -S inherit-network=y -S http ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm > /tmp/vfs-server-make.log 2>&1 &
	@sleep 2
	@echo "Running composed demo-writer..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-writer.wasm 2>&1 | grep -v "^\[RPC-ADAPTER\]"
	@echo ""
	@echo "Running composed demo-reader..."
	@wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/composed-demo-reader.wasm 2>&1 | grep -v "^\[RPC-ADAPTER\]"
	@echo ""
	@pkill -f vfs_rpc_server.wasm 2>/dev/null || true
	@echo "Test completed successfully!"

# =============================================================================
# Use Cases
# =============================================================================

# Run sensor pipeline usecase (VFS sharing demo)
run-usecase-sensor-pipeline: build-component-model-adapter build-usecase-sensor-pipeline
	@echo ""
	@echo "=============================================="
	@echo "  Use Case: Sensor Data Pipeline"
	@echo "=============================================="
	@echo ""
	@cd usecases/sensor-pipeline/runtime-linker && cargo run --release

# Build S3 sync logging usecase
build-usecase-s3-sync-logging:
	@echo "Building S3 sync logging usecase..."
	@cargo build -p logger --target wasm32-wasip2
	@echo "Built: usecases/s3-sync-logging/"

# Compose S3 sync logging with RPC adapter
compose-usecase-s3-sync-logging: build-rpc-adapter build-usecase-s3-sync-logging
	@echo "Composing logger with rpc-adapter..."
	@wac plug \
		--plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
		target/wasm32-wasip2/debug/logger.wasm \
		-o target/wasm32-wasip2/debug/composed-logger.wasm
	@echo "Composed: target/wasm32-wasip2/debug/composed-logger.wasm"

# Run S3 sync logging demo (requires LocalStack and VFS RPC server)
run-usecase-s3-sync-logging: compose-usecase-s3-sync-logging
	@echo ""
	@echo "=============================================="
	@echo "  Use Case: S3 Sync Logging"
	@echo "=============================================="
	@echo "Note: Requires LocalStack and VFS RPC server"
	@echo "See: usecases/s3-sync-logging/run-demo.sh"
	@echo ""

# Build HTTP cache demo usecase
build-usecase-http-cache:
	@echo "Building HTTP cache demo usecase..."
	@cargo build -p http-cache-handler --target wasm32-wasip2
	@cargo build -p http-cache-server --release
	@echo "Built: usecases/http-cache-demo/"

# Run HTTP cache demo (VFS cache sharing between WASM instances)
run-usecase-http-cache: build-component-model-adapter build-usecase-http-cache
	@echo ""
	@echo "=============================================="
	@echo "  Use Case: HTTP Cache Demo"
	@echo "=============================================="
	@echo ""
	@cd usecases/http-cache-demo/http-server && cargo run --release

# Build CI cache demo usecase
build-usecase-ci-cache:
	@echo "Building CI cache demo usecase..."
	@cargo build -p ci-job --target wasm32-wasip2
	@cargo build -p vfs-rpc-server --target wasm32-wasip2
	@cargo build -p rpc-adapter --target wasm32-wasip2
	@wac plug \
		--plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
		target/wasm32-wasip2/debug/ci-job.wasm \
		-o target/wasm32-wasip2/debug/ci-job-composed.wasm
	@echo "Built: usecases/ci-cache-demo/"

# Run CI cache demo (RPC-based VFS cache sharing between parallel CI jobs)
run-usecase-ci-cache: build-usecase-ci-cache
	@./usecases/ci-cache-demo/run-demo.sh

# Build image pipeline demo usecase (wac plug composition)
build-usecase-image-pipeline:
	@echo "Building image pipeline demo usecase..."
	@cargo build -p image-processor --target wasm32-wasip2
	@cargo build -p vfs-adapter --target wasm32-wasip2
	@wac plug \
		--plug target/wasm32-wasip2/debug/vfs_adapter.wasm \
		target/wasm32-wasip2/debug/image-processor.wasm \
		-o target/wasm32-wasip2/debug/image-processor-composed.wasm
	@echo "Built: usecases/image-pipeline-demo/"

# Run image pipeline demo (wac-composed VFS pipeline)
run-usecase-image-pipeline: build-usecase-image-pipeline
	@echo ""
	@echo "=============================================="
	@echo "  Use Case: Image Pipeline Demo"
	@echo "=============================================="
	@echo ""
	@wasmtime run target/wasm32-wasip2/debug/image-processor-composed.wasm

# =============================================================================
# Legacy (Deprecated)
# =============================================================================

# Build the C example WASM module
build-legacy-c:
	@echo "Building legacy C example..."
	@$(MAKE) -C deprecated/legacy-examples/c

# Build the Rust example WASM module
build-legacy-rust:
	@echo "Building legacy Rust example..."
	@cargo build --manifest-path deprecated/legacy-examples/rust/Cargo.toml --target-dir target --target wasm32-wasip1
	@echo "Built: target/wasm32-wasip1/debug/rust-example.wasm"

# Build all legacy examples
build-legacy-all: build-legacy-c build-legacy-rust

# Build C example (release)
build-legacy-c-release:
	@echo "Building legacy C example (release)..."
	@$(MAKE) -C deprecated/legacy-examples/c BUILD_MODE=release
	@echo "Release build complete: target/wasm32-wasip1/release/c_example.wasm"
	@ls -lh target/wasm32-wasip1/release/c_example.wasm

# Build Rust example (release)
build-legacy-rust-release:
	@echo "Building legacy Rust example (release)..."
	@cargo build --manifest-path deprecated/legacy-examples/rust/Cargo.toml --target-dir target --target wasm32-wasip1 --release
	@echo "Release build complete: target/wasm32-wasip1/release/rust-example.wasm"
	@ls -lh target/wasm32-wasip1/release/rust-example.wasm

# Build all legacy examples (release)
build-legacy-all-release: build-legacy-c-release build-legacy-rust-release

# Run C example
run-legacy-c: build-legacy-c
	@echo "Running legacy C example..."
	@wasmtime run target/wasm32-wasip1/debug/c_example.wasm

# Run Rust example
run-legacy-rust: build-legacy-rust
	@echo "Running legacy Rust example..."
	@wasmtime run target/wasm32-wasip1/debug/rust-example.wasm

# Run all legacy examples
run-legacy-all: run-legacy-c run-legacy-rust

# Run C example (release)
run-legacy-c-release: build-legacy-c-release
	@echo "Running legacy C example (release)..."
	@wasmtime run target/wasm32-wasip1/release/c_example.wasm

# Run Rust example (release)
run-legacy-rust-release: build-legacy-rust-release
	@echo "Running legacy Rust example (release)..."
	@wasmtime run target/wasm32-wasip1/release/rust-example.wasm

# Run all legacy examples (release)
run-legacy-all-release: run-legacy-c-release run-legacy-rust-release

# =============================================================================
# Utility
# =============================================================================

# Check build prerequisites
check-prereqs:
	@echo "Checking build prerequisites..."
	@echo -n "WASI-libc: "
	@if brew list wasi-libc >/dev/null 2>&1; then echo "installed"; else echo "missing"; fi
	@echo -n "LLVM: "
	@if brew list llvm >/dev/null 2>&1; then echo "installed"; else echo "missing"; fi
	@echo -n "wasm32-wasip1 target: "
	@if rustup target list --installed | grep -q wasm32-wasip1; then echo "installed"; else echo "missing"; fi
	@echo -n "wasm32-wasip2 target: "
	@if rustup target list --installed | grep -q wasm32-wasip2; then echo "installed"; else echo "missing"; fi
	@echo -n "wasmtime: "
	@if command -v wasmtime >/dev/null 2>&1; then echo "available"; else echo "missing"; fi
	@echo -n "wac: "
	@if command -v wac >/dev/null 2>&1; then echo "available"; else echo "missing"; fi

# Install prerequisites
install-prereqs:
	@echo "Installing prerequisites..."
	@brew install wasi-libc llvm
	@rustup target add wasm32-wasip1 wasm32-wasip2
	@cargo install wac-cli wasmtime-cli
	@echo "Prerequisites installed"

# Show file information
info:
	@echo "halycon - WebAssembly Component Model VFS"
	@echo "=========================================="
	@echo ""
	@if [ -f target/wasm32-wasip2/debug/vfs_adapter.wasm ]; then \
		echo "VFS Adapter (Component Model):"; \
		echo "  File: target/wasm32-wasip2/debug/vfs_adapter.wasm"; \
		echo "  Size: $$(ls -lh target/wasm32-wasip2/debug/vfs_adapter.wasm | awk '{print $$5}')"; \
		echo ""; \
	fi
	@if [ -f target/wasm32-wasip1/debug/c_example.wasm ]; then \
		echo "Legacy C Example:"; \
		echo "  File: target/wasm32-wasip1/debug/c_example.wasm"; \
		echo "  Size: $$(ls -lh target/wasm32-wasip1/debug/c_example.wasm | awk '{print $$5}')"; \
		echo ""; \
	fi
	@if [ -f target/wasm32-wasip1/debug/rust-example.wasm ]; then \
		echo "Legacy Rust Example:"; \
		echo "  File: target/wasm32-wasip1/debug/rust-example.wasm"; \
		echo "  Size: $$(ls -lh target/wasm32-wasip1/debug/rust-example.wasm | awk '{print $$5}')"; \
		echo ""; \
	fi

# Benchmark/performance test
benchmark: build-legacy-all
	@echo "Running performance benchmarks..."
	@echo ""
	@echo "Legacy C Example:"
	@time wasmtime run target/wasm32-wasip1/debug/c_example.wasm >/dev/null
	@echo ""
	@echo "Legacy Rust Example:"
	@time wasmtime run target/wasm32-wasip1/debug/rust-example.wasm >/dev/null

# =============================================================================
# Help
# =============================================================================

help:
	@echo "halycon - WebAssembly Component Model Filesystem"
	@echo "================================================="
	@echo ""
	@echo "Component Model:"
	@echo "  make demo-component-model-dynamic       - Run dynamic linking demo"
	@echo "  make demo-component-model-static        - Run static composition demos"
	@echo ""
	@echo "  Build:"
	@echo "    make build-component-model-adapter    - Build VFS adapter"
	@echo "    make build-component-model-rust       - Build Rust component"
	@echo "    make build-component-model-c          - Build C component"
	@echo "    make build-component-model-all        - Build all components"
	@echo "    make build-component-model-linker     - Build runtime linker host"
	@echo ""
	@echo "  Static Composition:"
	@echo "    make compose-component-model-rust     - Compose Rust with VFS adapter"
	@echo "    make compose-component-model-c        - Compose C with VFS adapter"
	@echo "    make run-component-model-static-rust  - Run composed Rust"
	@echo "    make run-component-model-static-c     - Run composed C"
	@echo ""
	@echo "RPC (requires server running):"
	@echo "  make build-rpc-server                   - Build VFS RPC server"
	@echo "  make build-rpc-demos                    - Build demo applications"
	@echo "  make build-rpc-all                      - Build all RPC components"
	@echo "  make start-rpc-server                   - Start VFS RPC server"
	@echo "  make stop-rpc-server                    - Stop VFS RPC server"
	@echo "  make run-rpc-demo-writer                - Run demo-writer"
	@echo "  make run-rpc-demo-reader                - Run demo-reader"
	@echo "  make run-rpc-demo-std-fs                - Run demo-std-fs"
	@echo ""
	@echo "RPC Composed (no native binary needed):"
	@echo "  make build-rpc-adapter                  - Build rpc-adapter"
	@echo "  make compose-rpc-demos                  - Compose demos with rpc-adapter"
	@echo "  make build-rpc-composed                 - Build all composed components"
	@echo "  make run-rpc-composed-writer            - Run composed demo-writer"
	@echo "  make run-rpc-composed-reader            - Run composed demo-reader"
	@echo "  make run-rpc-composed-test              - Run full composed test"
	@echo ""
	@echo "Use Cases:"
	@echo "  make build-usecase-sensor-pipeline      - Build sensor pipeline"
	@echo "  make run-usecase-sensor-pipeline        - Run sensor pipeline demo"
	@echo "  make build-usecase-s3-sync-logging      - Build S3 sync logging"
	@echo "  make compose-usecase-s3-sync-logging    - Compose with VFS adapter"
	@echo "  make run-usecase-s3-sync-logging        - Run S3 sync logging demo"
	@echo ""
	@echo "Legacy (Deprecated, wasm32-wasip1):"
	@echo "  make build-legacy-c                     - Build C example"
	@echo "  make build-legacy-rust                  - Build Rust example"
	@echo "  make build-legacy-all                   - Build all legacy examples"
	@echo "  make build-legacy-c-release             - Build C example (release)"
	@echo "  make build-legacy-rust-release          - Build Rust example (release)"
	@echo "  make run-legacy-c                       - Run C example"
	@echo "  make run-legacy-rust                    - Run Rust example"
	@echo "  make run-legacy-all                     - Run all legacy examples"
	@echo "  make run-legacy-c-release               - Run C example (release)"
	@echo "  make run-legacy-rust-release            - Run Rust example (release)"
	@echo ""
	@echo "Utility:"
	@echo "  make build                              - Build fs-core library"
	@echo "  make build-release                      - Build library (release)"
	@echo "  make clean                              - Clean build artifacts"
	@echo "  make check-prereqs                      - Check prerequisites"
	@echo "  make install-prereqs                    - Install prerequisites"
	@echo "  make info                               - Show build info"
	@echo "  make benchmark                          - Run benchmarks"
	@echo "  make help                               - Show this help"
	@echo ""
	@echo "Documentation:"
	@echo "  README.md                                           - Project overview"
	@echo "  examples/component-model/runtime-linker/README.md - Dynamic linking"
