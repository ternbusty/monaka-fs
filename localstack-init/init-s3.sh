#!/bin/bash
# LocalStack initialization script
# Creates S3 bucket for VFS persistence

echo "Initializing S3 bucket for VFS..."

awslocal s3 mb s3://test-vfs-bucket

echo "S3 bucket 'test-vfs-bucket' created successfully"
