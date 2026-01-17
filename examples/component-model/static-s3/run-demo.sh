#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"

BUCKET_NAME="static-s3-demo"
LOCALSTACK_URL="http://localhost:4566"

echo "=== Static Composition + S3 Sync Demo ==="
echo ""

# Check dependencies
command -v wasmtime >/dev/null 2>&1 || { echo "Error: wasmtime not found"; exit 1; }
command -v wac >/dev/null 2>&1 || { echo "Error: wac not found"; exit 1; }
command -v awslocal >/dev/null 2>&1 || { echo "Error: awslocal not found (pip install localstack)"; exit 1; }

# Build vfs-adapter with S3 sync
echo "1. Building vfs-adapter with S3 sync..."
cargo build -p vfs-adapter --target wasm32-wasip2 --features s3-sync --manifest-path "$ROOT_DIR/Cargo.toml"

# Build demo app
echo ""
echo "2. Building demo app..."
cargo build --manifest-path "$SCRIPT_DIR/Cargo.toml" --target wasm32-wasip2

DEMO_WASM="$SCRIPT_DIR/target/wasm32-wasip2/debug/static-s3-demo.wasm"
ADAPTER_WASM="$ROOT_DIR/target/wasm32-wasip2/debug/vfs_adapter.wasm"
COMPOSED_WASM="$ROOT_DIR/target/wasm32-wasip2/debug/static-s3-composed.wasm"

# Compose
echo ""
echo "3. Composing with wac plug..."
wac plug \
    --plug "$ADAPTER_WASM" \
    "$DEMO_WASM" \
    -o "$COMPOSED_WASM"

echo "   Created: $COMPOSED_WASM"

# Check LocalStack
echo ""
echo "4. Checking LocalStack..."
if ! curl -s "$LOCALSTACK_URL/_localstack/health" > /dev/null 2>&1; then
    echo "   LocalStack not running. Starting..."
    docker run -d --name localstack-static-s3 -p 4566:4566 localstack/localstack 2>/dev/null || true
    echo "   Waiting for LocalStack to be ready..."
    sleep 5
fi

# Create bucket
echo ""
echo "5. Creating S3 bucket..."
awslocal s3 mb "s3://$BUCKET_NAME" 2>/dev/null || echo "   Bucket already exists"

# Run demo
echo ""
echo "6. Running demo..."
echo "----------------------------------------"
wasmtime run -S inherit-network=y -S http \
    --env "VFS_S3_BUCKET=$BUCKET_NAME" \
    --env "VFS_S3_PREFIX=demo/" \
    --env "VFS_S3_SYNC_MODE=realtime" \
    --env "AWS_ENDPOINT_URL=$LOCALSTACK_URL" \
    --env "AWS_ACCESS_KEY_ID=test" \
    --env "AWS_SECRET_ACCESS_KEY=test" \
    --env "AWS_REGION=us-east-1" \
    "$COMPOSED_WASM"
echo "----------------------------------------"

# Verify S3 sync
echo ""
echo "7. Verifying S3 sync..."
echo ""
echo "Files in S3:"
awslocal s3 ls "s3://$BUCKET_NAME/demo/" --recursive 2>/dev/null || echo "   (no files found)"

echo ""
echo "Content of config.json:"
awslocal s3 cp "s3://$BUCKET_NAME/demo/files/data/config.json" - 2>/dev/null || echo "   (not found)"

echo ""
echo ""
echo "=== Demo Complete ==="
