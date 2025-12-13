# examples/Makefile
# This is a proxy Makefile that delegates to the root Makefile
# Allows running 'make demo' from the examples/ directory

.PHONY: demo demo-dynamic demo-static help all
.PHONY: build-vfs-adapter build-component-rust build-component-c build-components
.PHONY: build-runtime-linker compose-static-rust compose-static-c
.PHONY: run-static-rust run-static-c

# Default target
all: demo

# Proxy all targets to root Makefile
demo:
	@$(MAKE) -C .. demo

demo-dynamic:
	@$(MAKE) -C .. demo-dynamic

demo-static:
	@$(MAKE) -C .. demo-static

build-vfs-adapter:
	@$(MAKE) -C .. build-vfs-adapter

build-component-rust:
	@$(MAKE) -C .. build-component-rust

build-component-c:
	@$(MAKE) -C .. build-component-c

build-components:
	@$(MAKE) -C .. build-components

build-runtime-linker:
	@$(MAKE) -C .. build-runtime-linker

compose-static-rust:
	@$(MAKE) -C .. compose-static-rust

compose-static-c:
	@$(MAKE) -C .. compose-static-c

run-static-rust:
	@$(MAKE) -C .. run-static-rust

run-static-c:
	@$(MAKE) -C .. run-static-c

help:
	@echo "Examples Directory Makefile"
	@echo "==========================="
	@echo ""
	@echo "This Makefile delegates to the root Makefile."
	@echo "All commands work the same way from this directory."
	@echo ""
	@$(MAKE) -C .. help

# Catch-all: proxy to root
%:
	@$(MAKE) -C .. $@
