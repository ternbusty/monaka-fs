# C Example - Pure Makefile Build (No Cargo)
# Builds c_example.wasm using only rustc and clang

# Configuration
TARGET := wasm32-wasip1
BUILD_MODE := debug
WASI_SYSROOT := $(shell brew --prefix wasi-libc 2>/dev/null)/share/wasi-sysroot
ifeq ($(WASI_SYSROOT),/share/wasi-sysroot)
	WASI_SYSROOT := /opt/homebrew/Cellar/wasi-libc/26/share/wasi-sysroot
endif
CLANG := $(shell brew --prefix llvm 2>/dev/null)/bin/clang
ifeq ($(CLANG),/bin/clang)
	CLANG := /opt/homebrew/opt/llvm/bin/clang
endif

# Directories
ROOT_DIR := ../..
TARGET_DIR := $(ROOT_DIR)/target/$(TARGET)/$(BUILD_MODE)
DEPS_DIR := $(TARGET_DIR)/deps
FS_CORE_DIR := $(ROOT_DIR)/fs-core
FS_WASM_DIR := $(ROOT_DIR)/fs-wasm

# Output files
FS_CORE_RLIB := $(DEPS_DIR)/libfs_core.rlib
FS_WASM_RLIB := $(DEPS_DIR)/libfs_wasm.rlib
C_OBJECT := main.o
FINAL_WASM := $(TARGET_DIR)/c_example.wasm

# Compiler flags
RUSTC_FLAGS := --edition=2024 \
	--target $(TARGET) \
	-C opt-level=1 \
	-C embed-bitcode=no \
	-C debuginfo=2

CLANG_FLAGS := --target=$(TARGET) \
	--sysroot=$(WASI_SYSROOT) \
	-nostdlib \
	-c

# Default target
.PHONY: all
all: $(FINAL_WASM)

# Create directories
$(DEPS_DIR):
	@mkdir -p $(DEPS_DIR)

# Step 1 & 2: Build fs-core and fs-wasm libraries using cargo
# This ensures all dependencies (including wasi crate) are resolved properly
.PHONY: build-libs
build-libs: $(DEPS_DIR)
	@echo "Building fs-core..."
	@echo "Building fs-wasm..."
	@cd $(ROOT_DIR) && cargo build -p fs-core -p fs-wasm --target $(TARGET) $(if $(findstring release,$(BUILD_MODE)),--release,)

$(FS_CORE_RLIB): build-libs
$(FS_WASM_RLIB): build-libs

# Step 3: Compile C code to WASM object
$(C_OBJECT): main.c
	@echo "Compiling C code..."
	@$(CLANG) $(CLANG_FLAGS) \
		-o $(C_OBJECT) \
		main.c

# Step 4: Build final WASM binary
# Uses stub.rs (minimal Rust stub) with C main() from main.c
$(FINAL_WASM): build-libs $(C_OBJECT)
	@echo "Linking final WASM binary..."
	@rustc --crate-name c_example \
		$(RUSTC_FLAGS) \
		--crate-type bin \
		--emit=dep-info,link \
		--out-dir $(TARGET_DIR) \
		-L dependency=$(ROOT_DIR)/target/$(TARGET)/$(BUILD_MODE)/deps \
		--extern fs_wasm=$(ROOT_DIR)/target/$(TARGET)/$(BUILD_MODE)/deps/libfs_wasm.rlib \
		-C link-arg=$(C_OBJECT) \
		stub.rs
	@echo "Build complete: $(FINAL_WASM)"

# Clean build artifacts
.PHONY: clean
clean:
	@echo "Cleaning build artifacts..."
	@rm -f $(C_OBJECT)
	@rm -f $(FINAL_WASM)
	@echo "Clean complete"
	@echo "Note: Run 'make clean' from repository root to clean Rust libraries"

# Run the WASM binary
.PHONY: run
run: $(FINAL_WASM)
	@echo "Running c_example.wasm..."
	@wasmtime run $(FINAL_WASM)

# Show build information
.PHONY: info
info:
	@echo "C Example Build Configuration"
	@echo "=============================="
	@echo "Target:       $(TARGET)"
	@echo "Build Mode:   $(BUILD_MODE)"
	@echo "WASI Sysroot: $(WASI_SYSROOT)"
	@echo "Clang:        $(CLANG)"
	@echo ""
	@echo "Build Artifacts:"
	@echo "  fs-core:    $(FS_CORE_RLIB)"
	@echo "  fs-wasm:    $(FS_WASM_RLIB)"
	@echo "  C object:   $(C_OBJECT)"
	@echo "  Final WASM: $(FINAL_WASM)"

# Help target
.PHONY: help
help:
	@echo "C Example Makefile (No Cargo)"
	@echo "=============================="
	@echo ""
	@echo "Targets:"
	@echo "  make        - Build c_example.wasm"
	@echo "  make clean  - Remove build artifacts"
	@echo "  make run    - Build and run the WASM module"
	@echo "  make info   - Show build configuration"
	@echo "  make help   - Show this help message"
	@echo ""
	@echo "Build Process:"
	@echo "  1. Build fs-core as .rlib (Rust library)"
	@echo "  2. Build fs-wasm as .rlib (depends on fs-core)"
	@echo "  3. Compile main.c to main.o (WASM object)"
	@echo "  4. Link stub.rs + C object + libraries → c_example.wasm"
	@echo ""
	@echo "This Makefile builds the C example without using cargo."
	@echo "Only rustc and clang are used directly."
