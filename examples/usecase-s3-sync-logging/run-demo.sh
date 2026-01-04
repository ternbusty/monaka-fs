#!/bin/bash
# S3 Sync Logging Demo
#
# Demonstrates multiple WASM app replicas writing logs concurrently
# with automatic S3 synchronization via vfs-rpc-server.
#
# Prerequisites:
#   - LocalStack running on localhost:4566
#   - AWS CLI
#   - wasmtime
#   - wac-cli
#
# Usage:
#   ./examples/usecase-s3-sync-logging/run-demo.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"
cd "$PROJECT_DIR"

# Configuration
BUCKET_NAME="vfs-logs-demo"
S3_PREFIX="demo/"
AWS_REGION="us-east-1"
AWS_ENDPOINT_URL="http://localhost:4566"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }
log_step() { echo -e "${BLUE}[STEP]${NC} $1"; }

SERVER_LOG=/tmp/vfs-server-s3-demo.log

cleanup() {
    log_info "Cleaning up..."
    pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
}

trap cleanup EXIT

echo "=============================================="
echo "  S3 Sync Logging Demo"
echo "  Multiple replicas writing to shared VFS"
echo "=============================================="
echo ""

# Step 1: Create S3 bucket
log_step "1. Creating S3 bucket..."
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_DEFAULT_REGION=$AWS_REGION

aws --endpoint-url=$AWS_ENDPOINT_URL s3 mb s3://$BUCKET_NAME 2>/dev/null || \
    log_warn "Bucket already exists or creation failed"

# Step 2: Build components
log_step "2. Building WASM components..."
cargo build --target wasm32-wasip2 \
    -p vfs-rpc-server \
    -p rpc-adapter \
    -p logger

# Step 3: Compose logger with rpc-adapter
log_step "3. Composing logger with rpc-adapter..."
wac plug \
    --plug target/wasm32-wasip2/debug/rpc_adapter.wasm \
    target/wasm32-wasip2/debug/logger.wasm \
    -o target/wasm32-wasip2/debug/composed-logger.wasm
log_info "Composed: logger"

# Step 4: Start vfs-rpc-server with S3 config
log_step "4. Starting VFS RPC Server with S3 sync..."
pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
sleep 1

VFS_S3_BUCKET=$BUCKET_NAME \
VFS_S3_PREFIX=$S3_PREFIX \
AWS_REGION=$AWS_REGION \
AWS_ENDPOINT_URL=$AWS_ENDPOINT_URL \
AWS_ACCESS_KEY_ID=test \
AWS_SECRET_ACCESS_KEY=test \
wasmtime run -S inherit-network=y -S http \
    ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm > "$SERVER_LOG" 2>&1 &
SERVER_PID=$!
sleep 3

if ! ps -p $SERVER_PID > /dev/null 2>&1; then
    log_error "Server failed to start!"
    cat "$SERVER_LOG"
    exit 1
fi
log_info "Server started (PID: $SERVER_PID)"

# Step 5: Run 3 logger replicas in parallel
log_step "5. Running 3 logger replicas in parallel..."

REPLICA_ID=1 wasmtime run -S inherit-network=y \
    ./target/wasm32-wasip2/debug/composed-logger.wasm 2>&1 | \
    grep -v "^\[RPC-ADAPTER\]" &
PID1=$!

REPLICA_ID=2 wasmtime run -S inherit-network=y \
    ./target/wasm32-wasip2/debug/composed-logger.wasm 2>&1 | \
    grep -v "^\[RPC-ADAPTER\]" &
PID2=$!

REPLICA_ID=3 wasmtime run -S inherit-network=y \
    ./target/wasm32-wasip2/debug/composed-logger.wasm 2>&1 | \
    grep -v "^\[RPC-ADAPTER\]" &
PID3=$!

# Wait for all replicas to complete
wait $PID1 $PID2 $PID3
log_info "All replicas completed"

# Step 6: Wait for S3 sync
log_step "6. Waiting for S3 sync to complete (10 seconds)..."
sleep 10

# Step 7: Verify logs in S3
log_step "7. Verifying logs in S3..."
echo ""
echo "=== S3 Objects ==="
aws --endpoint-url=$AWS_ENDPOINT_URL s3 ls s3://$BUCKET_NAME/$S3_PREFIX --recursive

echo ""
echo "=== Log Contents (/logs/app.log) ==="
aws --endpoint-url=$AWS_ENDPOINT_URL s3 cp \
    s3://$BUCKET_NAME/${S3_PREFIX}files/logs/app.log - 2>/dev/null || \
    echo "(not found - check server log below)"

# Step 8: Show server log
echo ""
log_step "8. Server log (last 30 lines):"
tail -30 "$SERVER_LOG"

echo ""
echo "=============================================="
log_info "Demo completed!"
echo "=============================================="
echo ""
echo "Summary:"
echo "  - 3 logger replicas wrote logs concurrently"
echo "  - All wrote to shared /logs/app.log"
echo "  - Logs were synced to LocalStack S3"
echo "  - Bucket: $BUCKET_NAME"
echo "  - S3 Key: ${S3_PREFIX}files/logs/app.log"
