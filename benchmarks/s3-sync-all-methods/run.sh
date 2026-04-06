#!/bin/bash
# S3 Sync All Methods Benchmark Runner
# Usage: ./run.sh
#
# Compares four S3 synchronization methods in S3 passthrough mode:
#   1. s3fs-fuse - Direct S3 mount via FUSE
#   2. vfs-host  - Host trait with S3 sync
#   3. wac-plug  - WASM composition with vfs-adapter
#   4. RPC       - rpc-adapter + vfs-rpc-server
#
# Requires a .env file with S3/GCS credentials.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WASM_DIR="$SCRIPT_DIR/wasm"
BIN_DIR="$SCRIPT_DIR/bin"
S3FS_MOUNT="/tmp/s3fs-bench"
ENV_FILE="$SCRIPT_DIR/.env"

export PATH="$HOME/.wasmtime/bin:$HOME/.cargo/bin:$PATH"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Load .env file (required)
if [ ! -f "$ENV_FILE" ]; then
    echo -e "${RED}[ERROR] .env file not found at $ENV_FILE${NC}"
    echo ""
    echo "Create a .env file with S3/GCS credentials:"
    echo "  AWS_ACCESS_KEY_ID=your-key"
    echo "  AWS_SECRET_ACCESS_KEY=your-secret"
    echo "  AWS_REGION=your-region"
    echo "  AWS_ENDPOINT_URL=https://storage.googleapis.com  # for GCS"
    echo "  VFS_S3_BUCKET=your-bucket"
    echo "  VFS_S3_PREFIX=benchmark/"
    exit 1
fi

echo "Loading credentials from $ENV_FILE"
set -a
source "$ENV_FILE"
set +a

S3FS_URL="$AWS_ENDPOINT_URL"
S3_BUCKET="$VFS_S3_BUCKET"

echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  Benchmark: S3 Sync - All Methods${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""
echo "Mode: Full S3 Passthrough"
echo "  - Sync: realtime (immediate S3 PUT on write)"
echo "  - Read: s3 (read-through from S3)"
echo "  - Metadata: s3 (HEAD request on open)"
echo ""
echo "Bucket: $VFS_S3_BUCKET"
echo ""

# Verify required tools
for cmd in s3fs bc aws wasmtime; do
    if ! command -v $cmd &> /dev/null; then
        echo -e "${RED}[ERROR] $cmd not found${NC}"
        exit 1
    fi
done

# Verify files
for f in "$WASM_DIR/bench-app.wasm" "$BIN_DIR/bench-runtime"; do
    if [ ! -f "$f" ]; then
        echo -e "${RED}[ERROR] $(basename "$f") not found${NC}"
        exit 1
    fi
done

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    if mountpoint -q "$S3FS_MOUNT" 2>/dev/null; then
        fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
    fi
    rm -rf "$S3FS_MOUNT"
    pkill -f "vfs-rpc-server" 2>/dev/null || true
}
trap cleanup EXIT

export VFS_SYNC_MODE=realtime
export VFS_READ_MODE=s3
export VFS_METADATA_MODE=s3

clear_s3() {
    aws s3 rm "s3://$S3_BUCKET/${VFS_S3_PREFIX}" --recursive 2>/dev/null || true
    sleep 1
}

# ==========================================
#   Phase 1: s3fs-fuse
# ==========================================
echo ""
echo -e "${YELLOW}=== Phase 1: s3fs-fuse ===${NC}"
echo ""

if mountpoint -q "$S3FS_MOUNT" 2>/dev/null; then
    fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
    sleep 1
fi
rm -rf "$S3FS_MOUNT"

echo "${AWS_ACCESS_KEY_ID}:${AWS_SECRET_ACCESS_KEY}" > ~/.passwd-s3fs
chmod 600 ~/.passwd-s3fs

mkdir -p "$S3FS_MOUNT"
s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
    -o passwd_file=~/.passwd-s3fs \
    -o url="$S3FS_URL" \
    -o sigv4 \
    -o allow_other 2>/dev/null || \
s3fs "$S3_BUCKET" "$S3FS_MOUNT" \
    -o passwd_file=~/.passwd-s3fs \
    -o url="$S3FS_URL" \
    -o sigv4
sleep 2

S3FS_START=$(date +%s%N)
S3FS_RESULTS=$(wasmtime run --dir="${S3FS_MOUNT}::/data" "$WASM_DIR/bench-app.wasm" 2>&1)
S3FS_END=$(date +%s%N)
S3FS_TOTAL_MS=$(echo "scale=3; ($S3FS_END - $S3FS_START) / 1000000" | bc)
echo "$S3FS_RESULTS"
echo "[SYNC] s3fs total: ${S3FS_TOTAL_MS}ms"

fusermount -u "$S3FS_MOUNT" 2>/dev/null || true
sleep 1

# ==========================================
#   Phase 2: vfs-host
# ==========================================
echo ""
echo -e "${YELLOW}=== Phase 2: vfs-host ===${NC}"
echo ""

clear_s3

VFSHOST_START=$(date +%s%N)
VFSHOST_RESULTS=$(VFS_S3_BUCKET="$VFS_S3_BUCKET" \
VFS_S3_PREFIX="$VFS_S3_PREFIX" \
VFS_SYNC_MODE=realtime \
VFS_READ_MODE=s3 \
VFS_METADATA_MODE=s3 \
AWS_ENDPOINT_URL="$AWS_ENDPOINT_URL" \
AWS_REGION="$AWS_REGION" \
AWS_ACCESS_KEY_ID="$AWS_ACCESS_KEY_ID" \
AWS_SECRET_ACCESS_KEY="$AWS_SECRET_ACCESS_KEY" \
"$BIN_DIR/bench-runtime" "$WASM_DIR/bench-app.wasm" 2>&1)
VFSHOST_END=$(date +%s%N)
VFSHOST_TOTAL_MS=$(echo "scale=3; ($VFSHOST_END - $VFSHOST_START) / 1000000" | bc)
echo "$VFSHOST_RESULTS"
echo "[SYNC] vfs-host total: ${VFSHOST_TOTAL_MS}ms"

# ==========================================
#   Phase 3: wac-plug
# ==========================================
echo ""
echo -e "${YELLOW}=== Phase 3: wac-plug ===${NC}"
echo ""

clear_s3

WAC_TOTAL_MS="N/A"
if [ ! -f "$WASM_DIR/bench-wac.wasm" ]; then
    echo -e "${RED}[SKIP] bench-wac.wasm not found${NC}"
else
    WAC_START=$(date +%s%N)
    WAC_RESULTS=$(wasmtime run \
        -S http \
        --env VFS_S3_BUCKET="$VFS_S3_BUCKET" \
        --env VFS_S3_PREFIX="$VFS_S3_PREFIX" \
        --env VFS_SYNC_MODE=realtime \
        --env VFS_READ_MODE=s3 \
        --env VFS_METADATA_MODE=s3 \
        --env AWS_ENDPOINT_URL="$AWS_ENDPOINT_URL" \
        --env AWS_REGION="$AWS_REGION" \
        --env AWS_ACCESS_KEY_ID="$AWS_ACCESS_KEY_ID" \
        --env AWS_SECRET_ACCESS_KEY="$AWS_SECRET_ACCESS_KEY" \
        "$WASM_DIR/bench-wac.wasm" 2>&1)
    WAC_END=$(date +%s%N)
    WAC_TOTAL_MS=$(echo "scale=3; ($WAC_END - $WAC_START) / 1000000" | bc)
    echo "$WAC_RESULTS"
    echo "[SYNC] wac-plug total: ${WAC_TOTAL_MS}ms"
fi

# ==========================================
#   Phase 4: RPC
# ==========================================
echo ""
echo -e "${YELLOW}=== Phase 4: RPC ===${NC}"
echo ""

clear_s3

RPC_TOTAL_MS="N/A"
if [ ! -f "$WASM_DIR/vfs-rpc-server.wasm" ] || [ ! -f "$WASM_DIR/bench-rpc.wasm" ]; then
    echo -e "${RED}[SKIP] RPC components not found${NC}"
else
    echo "Starting RPC server..."
    wasmtime run \
        -S inherit-network=y \
        -S http \
        --env VFS_S3_BUCKET="$VFS_S3_BUCKET" \
        --env VFS_S3_PREFIX="$VFS_S3_PREFIX" \
        --env VFS_SYNC_MODE=realtime \
        --env VFS_READ_MODE=s3 \
        --env VFS_METADATA_MODE=s3 \
        --env AWS_ENDPOINT_URL="$AWS_ENDPOINT_URL" \
        --env AWS_REGION="$AWS_REGION" \
        --env AWS_ACCESS_KEY_ID="$AWS_ACCESS_KEY_ID" \
        --env AWS_SECRET_ACCESS_KEY="$AWS_SECRET_ACCESS_KEY" \
        "$WASM_DIR/vfs-rpc-server.wasm" &
    RPC_SERVER_PID=$!
    sleep 2

    if ! kill -0 $RPC_SERVER_PID 2>/dev/null; then
        echo -e "${RED}[ERROR] RPC server failed to start${NC}"
    else
        RPC_START=$(date +%s%N)
        RPC_RESULTS=$(wasmtime run \
            -S inherit-network=y \
            "$WASM_DIR/bench-rpc.wasm" 2>&1)
        RPC_END=$(date +%s%N)
        RPC_TOTAL_MS=$(echo "scale=3; ($RPC_END - $RPC_START) / 1000000" | bc)
        echo "$RPC_RESULTS"
        echo "[SYNC] RPC total: ${RPC_TOTAL_MS}ms"

        kill $RPC_SERVER_PID 2>/dev/null || true
    fi
fi

# ==========================================
#   Summary
# ==========================================
echo ""
echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}  Results Summary${NC}"
echo -e "${GREEN}==========================================${NC}"
echo ""
echo "Format: operation,size,time_ms,throughput_mb_s"
echo ""

echo "--- s3fs-fuse ---"
echo "$S3FS_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //' || true
echo ""
echo "--- vfs-host ---"
echo "$VFSHOST_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //' || true
echo ""
if [ -n "${WAC_RESULTS:-}" ]; then
    echo "--- wac-plug ---"
    echo "$WAC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //' || true
    echo ""
fi
if [ -n "${RPC_RESULTS:-}" ]; then
    echo "--- RPC ---"
    echo "$RPC_RESULTS" | grep "^\[RESULT\]" | sed 's/\[RESULT\] //' || true
    echo ""
fi

echo -e "${GREEN}=== E2E Total Time ===${NC}"
echo "s3fs-fuse:  ${S3FS_TOTAL_MS}ms"
echo "vfs-host:   ${VFSHOST_TOTAL_MS}ms"
echo "wac-plug:   ${WAC_TOTAL_MS}ms"
echo "RPC:        ${RPC_TOTAL_MS}ms"

echo ""
echo -e "${GREEN}=== Benchmark Complete ===${NC}"
