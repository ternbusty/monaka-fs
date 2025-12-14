#!/bin/bash
# Test S3 persistence with LocalStack
#
# Usage:
#   ./scripts/test-s3-persistence.sh          # Full test (write + restart + read)
#   ./scripts/test-s3-persistence.sh server   # Start server only
#   ./scripts/test-s3-persistence.sh clean    # Clean up S3 data

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_DIR"

# S3 configuration
export VFS_S3_BUCKET=test-vfs-bucket
export VFS_S3_PREFIX=vfs/
export AWS_ENDPOINT_URL=http://127.0.0.1:4566
export AWS_ACCESS_KEY_ID=test
export AWS_SECRET_ACCESS_KEY=test
export AWS_REGION=us-east-1

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Log files for server output
SERVER_LOG_1=/tmp/vfs-server-phase1.log
SERVER_LOG_2=/tmp/vfs-server-phase2.log

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Ensure LocalStack is running
ensure_localstack() {
    if ! docker compose ps 2>/dev/null | grep -q "localstack.*healthy"; then
        log_info "Starting LocalStack..."
        docker compose up -d
        sleep 10
    fi

    # Verify bucket exists
    if ! docker compose exec -T localstack awslocal s3 ls s3://$VFS_S3_BUCKET 2>/dev/null; then
        log_info "Creating S3 bucket..."
        docker compose exec -T localstack awslocal s3 mb s3://$VFS_S3_BUCKET
    fi
}

# Clean S3 data
clean_s3() {
    log_info "Cleaning S3 data..."
    docker compose exec -T localstack awslocal s3 rm s3://$VFS_S3_BUCKET/$VFS_S3_PREFIX --recursive 2>/dev/null || true
    log_info "S3 data cleaned"
}

# Start VFS RPC server with S3 persistence
start_server() {
    log_info "Starting VFS RPC Server with S3 persistence..."
    wasmtime run \
        -S inherit-network=y \
        -S http \
        --env VFS_S3_BUCKET=$VFS_S3_BUCKET \
        --env VFS_S3_PREFIX=$VFS_S3_PREFIX \
        --env AWS_ENDPOINT_URL=$AWS_ENDPOINT_URL \
        --env AWS_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID \
        --env AWS_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY \
        --env AWS_REGION=$AWS_REGION \
        ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm
}

# Start server in background and return PID
start_server_bg() {
    local log_file="${1:-/tmp/vfs-server.log}"
    wasmtime run \
        -S inherit-network=y \
        -S http \
        --env VFS_S3_BUCKET=$VFS_S3_BUCKET \
        --env VFS_S3_PREFIX=$VFS_S3_PREFIX \
        --env AWS_ENDPOINT_URL=$AWS_ENDPOINT_URL \
        --env AWS_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID \
        --env AWS_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY \
        --env AWS_REGION=$AWS_REGION \
        ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm > "$log_file" 2>&1 &
    echo $!
}

# Run demo-writer
run_writer() {
    log_info "Running demo-writer..."
    ./target/debug/rpc-fs-runner ./target/wasm32-wasip2/debug/demo-writer.wasm
}

# Run demo-reader
run_reader() {
    log_info "Running demo-reader..."
    ./target/debug/rpc-fs-runner ./target/wasm32-wasip2/debug/demo-reader.wasm
}

# Show S3 contents
show_s3_contents() {
    log_info "S3 bucket contents:"
    docker compose exec -T localstack awslocal s3 ls s3://$VFS_S3_BUCKET/$VFS_S3_PREFIX --recursive 2>&1 || true

    echo ""
    log_info "Snapshot content (first 500 chars):"
    docker compose exec -T localstack awslocal s3 cp s3://$VFS_S3_BUCKET/${VFS_S3_PREFIX}snapshot.json - 2>&1 | head -c 500 || true
    echo ""
}

# Kill any existing server processes
kill_existing_servers() {
    log_info "Killing any existing VFS server processes..."
    pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
    sleep 1
}

# Full test: write, restart, read
full_test() {
    echo "=============================================="
    echo "  VFS RPC Server S3 Persistence Test"
    echo "=============================================="
    echo ""

    kill_existing_servers
    ensure_localstack
    clean_s3

    # Phase 1: Write data
    echo ""
    log_info "=== Phase 1: Write data ==="
    SERVER_PID=$(start_server_bg "$SERVER_LOG_1")
    sleep 3

    run_writer
    sleep 2

    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true
    # Ensure server is fully terminated
    pkill -f "vfs_rpc_server.wasm" 2>/dev/null || true
    sleep 2

    echo ""
    show_s3_contents

    # Phase 2: Restart and read
    echo ""
    log_info "=== Phase 2: Restart server and read data ==="
    SERVER_PID=$(start_server_bg "$SERVER_LOG_2")
    sleep 3

    run_reader
    sleep 1

    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true

    echo ""
    log_info "=== Server logs (Phase 1) ==="
    cat "$SERVER_LOG_1"

    echo ""
    log_info "=== Server logs (Phase 2) ==="
    cat "$SERVER_LOG_2"

    echo ""
    log_info "=== Test completed ==="
}

# Main
case "${1:-}" in
    server)
        ensure_localstack
        start_server
        ;;
    clean)
        ensure_localstack
        clean_s3
        ;;
    *)
        ensure_localstack
        full_test
        ;;
esac
