# halycon Makefile

.PHONY: build build-release build-all build-native build-wasm build-wasm-release clean help
.PHONY: build-usecase-sensor-pipeline run-usecase-sensor-pipeline
.PHONY: build-usecase-s3-sync-logging compose-usecase-s3-sync-logging run-usecase-s3-sync-logging
.PHONY: build-usecase-http-cache run-usecase-http-cache
.PHONY: build-usecase-ci-cache run-usecase-ci-cache
.PHONY: build-usecase-image-pipeline run-usecase-image-pipeline
.PHONY: run-usecase-rpc-concurrent run-usecase-host-concurrent
.PHONY: check-prereqs install-prereqs info help

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
		-p sensor-ingest \
		-p sensor-process \
		-p logger
	@echo "WASM packages built (release)"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean
	@rm -f examples/*.composed.wasm
	@echo "Clean complete"

# =============================================================================
# Use Cases
# =============================================================================

# Build sensor pipeline usecase
build-usecase-sensor-pipeline:
	@echo "Building sensor pipeline usecase..."
	@cargo build -p sensor-ingest --target wasm32-wasip2
	@cargo build -p sensor-process --target wasm32-wasip2
	@cargo build -p sensor-pipeline-runner --release
	@echo "Built: usecases/sensor-pipeline/"

# Run sensor pipeline usecase (VFS sharing demo)
run-usecase-sensor-pipeline: build-usecase-sensor-pipeline
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
compose-usecase-s3-sync-logging: build-usecase-s3-sync-logging
	@echo "Building rpc-adapter..."
	@cargo build -p rpc-adapter --target wasm32-wasip2
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
run-usecase-http-cache: build-usecase-http-cache
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

# Run RPC concurrent append test
run-usecase-rpc-concurrent:
	@cd usecases/rpc-concurrent-append && ./run-test.sh

# Run Host Trait concurrent append test (true parallel)
run-usecase-host-concurrent:
	@cd usecases/host-concurrent-append && ./run-test.sh

# =============================================================================
# Utility
# =============================================================================

# Check build prerequisites
check-prereqs:
	@echo "Checking build prerequisites..."
	@echo -n "wasm32-wasip2 target: "
	@if rustup target list --installed | grep -q wasm32-wasip2; then echo "installed"; else echo "missing"; fi
	@echo -n "wasmtime: "
	@if command -v wasmtime >/dev/null 2>&1; then echo "available"; else echo "missing"; fi
	@echo -n "wac: "
	@if command -v wac >/dev/null 2>&1; then echo "available"; else echo "missing"; fi

# Install prerequisites
install-prereqs:
	@echo "Installing prerequisites..."
	@rustup target add wasm32-wasip2
	@cargo install wac-cli wasmtime-cli
	@echo "Prerequisites installed"

# Show file information
info:
	@echo "halycon - WebAssembly Component Model VFS"
	@echo "=========================================="
	@echo ""
	@if [ -f target/wasm32-wasip2/debug/vfs_adapter.wasm ]; then \
		echo "VFS Adapter:"; \
		echo "  File: target/wasm32-wasip2/debug/vfs_adapter.wasm"; \
		echo "  Size: $$(ls -lh target/wasm32-wasip2/debug/vfs_adapter.wasm | awk '{print $$5}')"; \
		echo ""; \
	fi

# =============================================================================
# Help
# =============================================================================

help:
	@echo "halycon - WebAssembly Component Model Filesystem"
	@echo "================================================="
	@echo ""
	@echo "Build:"
	@echo "  make build                              - Build fs-core library"
	@echo "  make build-release                      - Build library (release)"
	@echo "  make build-wasm                         - Build all WASM packages"
	@echo "  make build-wasm-release                 - Build all WASM packages (release)"
	@echo "  make build-all                          - Build everything"
	@echo "  make clean                              - Clean build artifacts"
	@echo ""
	@echo "Use Cases:"
	@echo "  make build-usecase-sensor-pipeline      - Build sensor pipeline"
	@echo "  make run-usecase-sensor-pipeline        - Run sensor pipeline demo"
	@echo "  make run-usecase-rpc-concurrent         - Run RPC concurrent append test"
	@echo "  make run-usecase-host-concurrent        - Run Host Trait concurrent test"
	@echo "  make build-usecase-s3-sync-logging      - Build S3 sync logging"
	@echo "  make compose-usecase-s3-sync-logging    - Compose with rpc-adapter"
	@echo "  make run-usecase-s3-sync-logging        - Run S3 sync logging demo"
	@echo "  make build-usecase-http-cache           - Build HTTP cache demo"
	@echo "  make run-usecase-http-cache             - Run HTTP cache demo"
	@echo "  make build-usecase-ci-cache             - Build CI cache demo"
	@echo "  make run-usecase-ci-cache               - Run CI cache demo"
	@echo "  make build-usecase-image-pipeline       - Build image pipeline demo"
	@echo "  make run-usecase-image-pipeline         - Run image pipeline demo"
	@echo ""
	@echo "Utility:"
	@echo "  make check-prereqs                      - Check prerequisites"
	@echo "  make install-prereqs                    - Install prerequisites"
	@echo "  make info                               - Show build info"
	@echo "  make help                               - Show this help"
	@echo ""
	@echo "Examples:"
	@echo "  See examples/ directory for build/run instructions per deployment method"
