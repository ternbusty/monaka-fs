#!/bin/bash
# VM Setup Script for Halycon Benchmarks
# Usage: ./scripts/vm-setup.sh
# This script installs required tools on the multipass VM

set -e

VM_NAME="composed-petrel"

echo "=== Halycon Benchmark VM Setup ==="
echo "VM: $VM_NAME"
echo ""

# Create setup script to run inside VM
SETUP_SCRIPT='
set -e

echo "=== Installing benchmark prerequisites ==="

# Update package list
sudo apt-get update

# Install wasmtime
echo ""
echo "Installing wasmtime..."
if ! command -v wasmtime &> /dev/null; then
    curl https://wasmtime.dev/install.sh -sSf | bash
    echo "export PATH=\"\$HOME/.wasmtime/bin:\$PATH\"" >> ~/.bashrc
    export PATH="$HOME/.wasmtime/bin:$PATH"
else
    echo "wasmtime already installed"
fi

# Install Docker
echo ""
echo "Installing Docker..."
if ! command -v docker &> /dev/null; then
    sudo apt-get install -y ca-certificates curl gnupg
    sudo install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    sudo chmod a+r /etc/apt/keyrings/docker.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
    sudo apt-get update
    sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
    sudo usermod -aG docker $USER
    echo "NOTE: Docker group added. You may need to log out and back in."
else
    echo "Docker already installed"
fi

# Install s3fs-fuse
echo ""
echo "Installing s3fs-fuse..."
if ! command -v s3fs &> /dev/null; then
    sudo apt-get install -y s3fs
else
    echo "s3fs already installed"
fi

# Install utilities
echo ""
echo "Installing utilities..."
sudo apt-get install -y bc time curl jq

# Create benchmark directory
mkdir -p ~/halycon-bench/{wasm,scripts,results}

# Verify installations
echo ""
echo "=== Verification ==="
export PATH="$HOME/.wasmtime/bin:$PATH"
wasmtime --version || echo "wasmtime: NOT FOUND (restart shell)"
docker --version || echo "docker: NOT FOUND"
s3fs --version 2>&1 | head -1 || echo "s3fs: NOT FOUND"

echo ""
echo "=== VM Setup Complete ==="
echo ""
echo "If this is your first time, please run:"
echo "  multipass shell composed-petrel"
echo "  exit"
echo "to apply Docker group changes."
'

# Execute setup script on VM
echo "Running setup on VM..."
multipass exec "$VM_NAME" -- bash -c "$SETUP_SCRIPT"

echo ""
echo "=== Host Setup Complete ==="
