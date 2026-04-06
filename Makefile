# monaka Makefile

.PHONY: build build-release build-all build-native build-wasm build-wasm-release build-cli clean help
.PHONY: check-prereqs install-prereqs info

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
		-p vfs-rpc-server
	@echo "WASM packages built"

# Build all WASM packages (release)
build-wasm-release:
	@echo "Building WASM packages (release)..."
	@cargo build --release --target wasm32-wasip2 \
		-p vfs-adapter \
		-p rpc-adapter \
		-p vfs-rpc-server
	@echo "WASM packages built (release)"

# Build monaka CLI (builds all required WASM first, then the native CLI)
build-cli:
	@echo "=== Building WASM components for CLI ==="
	@echo "Building vfs-adapter (S3 sync)..."
	@cargo build --release --target wasm32-wasip2 -p vfs-adapter --features s3-sync
	@mkdir -p target/wasm32-wasip2/s3-release
	@cp target/wasm32-wasip2/release/vfs_adapter.wasm target/wasm32-wasip2/s3-release/vfs_adapter.wasm
	@echo "Building vfs-rpc-server (S3 sync)..."
	@cargo build --release --target wasm32-wasip2 -p vfs-rpc-server --features s3-sync
	@cp target/wasm32-wasip2/release/vfs_rpc_server.wasm target/wasm32-wasip2/s3-release/vfs_rpc_server.wasm
	@echo "Building vfs-adapter, rpc-adapter, vfs-rpc-server (no S3)..."
	@cargo build --release --target wasm32-wasip2 -p vfs-adapter -p rpc-adapter -p vfs-rpc-server
	@echo "=== Building monaka CLI ==="
	@cargo build --release -p monaka
	@echo "CLI built: target/release/monaka"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean
	@rm -f examples/*.composed.wasm
	@echo "Clean complete"

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
	@echo "monaka - WebAssembly Component Model VFS"
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
	@echo "monaka - WebAssembly Component Model Filesystem"
	@echo "================================================="
	@echo ""
	@echo "Build:"
	@echo "  make build                              - Build fs-core library"
	@echo "  make build-release                      - Build library (release)"
	@echo "  make build-wasm                         - Build all WASM packages"
	@echo "  make build-wasm-release                 - Build all WASM packages (release)"
	@echo "  make build-all                          - Build everything"
	@echo "  make build-cli                          - Build monaka CLI (WASM + native)"
	@echo "  make clean                              - Clean build artifacts"
	@echo ""
	@echo "Utility:"
	@echo "  make check-prereqs                      - Check prerequisites"
	@echo "  make install-prereqs                    - Install prerequisites"
	@echo "  make info                               - Show build info"
	@echo "  make help                               - Show this help"
	@echo ""
	@echo "See examples/ and usecases/ for build/run instructions."
