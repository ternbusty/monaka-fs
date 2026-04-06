#!/bin/bash
# Monaka CLI Installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ternbusty/monaka-fs/main/scripts/install.sh | bash
#   curl -fsSL ... | bash -s -- --version v0.2.0
#
# Installs the monaka binary to /usr/local/bin (or ~/.local/bin if no sudo).

set -e

REPO="ternbusty/monaka-fs"
BINARY_NAME="monaka"
INSTALL_DIR="/usr/local/bin"
USE_SUDO="true"

# Parse arguments
VERSION="latest"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --help)
            echo "Usage: install.sh [--version VERSION]"
            echo ""
            echo "Options:"
            echo "  --version VERSION  Install a specific version (e.g., v0.2.0)"
            echo "                     Default: latest"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Darwin) OS_NAME="darwin" ;;
    Linux)  OS_NAME="linux" ;;
    *)
        echo "Error: Unsupported OS: $OS"
        exit 1
        ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)   ARCH_NAME="amd64" ;;
    aarch64|arm64)   ARCH_NAME="arm64" ;;
    *)
        echo "Error: Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

ARTIFACT="${BINARY_NAME}-${OS_NAME}-${ARCH_NAME}"

echo "Detected: ${OS_NAME}/${ARCH_NAME}"

# Resolve version
if [ "$VERSION" = "latest" ]; then
    echo "Fetching latest release..."
    RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"
else
    echo "Fetching release ${VERSION}..."
    RELEASE_URL="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
fi

# Get download URL
RELEASE_JSON=$(curl -fsSL "$RELEASE_URL") || {
    echo "Error: Failed to fetch release info"
    exit 1
}

DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o "\"browser_download_url\": \"[^\"]*${ARTIFACT}\"" | head -1 | cut -d'"' -f4)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "Error: No binary found for ${ARTIFACT}"
    echo "Available assets:"
    echo "$RELEASE_JSON" | grep "browser_download_url" | cut -d'"' -f4
    exit 1
fi

TAG=$(echo "$RELEASE_JSON" | grep -o '"tag_name": "[^"]*"' | head -1 | cut -d'"' -f4)
echo "Installing ${BINARY_NAME} ${TAG} (${OS_NAME}/${ARCH_NAME})..."

# Download
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

curl -fsSL -o "${TMP_DIR}/${BINARY_NAME}" "$DOWNLOAD_URL"
chmod +x "${TMP_DIR}/${BINARY_NAME}"

# Install
if [ -w "$INSTALL_DIR" ]; then
    USE_SUDO="false"
elif ! command -v sudo &> /dev/null; then
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
    USE_SUDO="false"
    echo "Note: Installing to ${INSTALL_DIR} (no sudo available)"
fi

if [ "$USE_SUDO" = "true" ]; then
    sudo install -m 755 "${TMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
else
    install -m 755 "${TMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
fi

echo ""
echo "Installed ${BINARY_NAME} ${TAG} to ${INSTALL_DIR}/${BINARY_NAME}"
echo ""
echo "Run '${BINARY_NAME} --help' to get started."
