#!/bin/bash
# Create S3 bucket for benchmarks

awslocal s3 mb s3://halycon-bench
echo "Created bucket: halycon-bench"
