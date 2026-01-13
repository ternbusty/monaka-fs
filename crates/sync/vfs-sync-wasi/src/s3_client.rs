//! S3 Client for WASI environment
//!
//! Provides S3 operations for per-file synchronization with bidirectional sync support.

use aws_config::BehaviorVersion;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use aws_smithy_async::rt::sleep::TokioSleep;

/// Multipart upload threshold (10MB)
const MULTIPART_THRESHOLD: usize = 10 * 1024 * 1024;
/// Part size for multipart upload (10MB)
const PART_SIZE: usize = 10 * 1024 * 1024;

use crate::wasi_http::ChunkedWasiHttpClient;

/// S3 object info from listing
#[derive(Debug, Clone)]
pub struct S3ObjectInfo {
    /// Key without prefix (VFS path)
    pub path: String,
    /// ETag (usually MD5)
    pub etag: String,
    /// Last modified timestamp (Unix epoch seconds)
    pub last_modified: u64,
    /// Size in bytes
    pub size: u64,
}

/// S3 client wrapper for VFS persistence
pub struct S3Storage {
    client: Client,
    bucket: String,
    prefix: String,
}

impl S3Storage {
    /// Create a new S3 storage client
    ///
    /// # Arguments
    /// * `bucket` - S3 bucket name
    /// * `prefix` - Key prefix for all objects (e.g., "vfs/")
    ///
    /// # Environment Variables
    /// * `AWS_ENDPOINT_URL` - Custom endpoint URL (e.g., http://localhost:4566 for LocalStack)
    /// * `AWS_REGION` - AWS region (default: us-east-1)
    pub async fn new(bucket: String, prefix: String) -> Self {
        let http_client = ChunkedWasiHttpClient::new();
        let sleep = TokioSleep::new();

        let mut config_loader = aws_config::defaults(BehaviorVersion::latest())
            .http_client(http_client)
            .sleep_impl(sleep);

        // Check for custom endpoint (LocalStack, MinIO, etc.)
        if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
            log::debug!("[s3] Using custom endpoint: {}", endpoint);
            config_loader = config_loader.endpoint_url(&endpoint);
        }

        let config = config_loader.load().await;

        // Create S3 client with path-style access for LocalStack compatibility
        let s3_config = aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(true)
            .build();

        let client = Client::from_conf(s3_config);

        Self {
            client,
            bucket,
            prefix,
        }
    }

    /// Get the full S3 key for a given path
    fn key(&self, path: &str) -> String {
        format!("{}{}", self.prefix, path.trim_start_matches('/'))
    }

    /// Delete a file from S3
    pub async fn delete_file(&self, path: &str) -> Result<(), S3Error> {
        let key = self.key(&format!("files{}", path));

        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| S3Error::Delete {
                key: key.clone(),
                message: e.to_string(),
            })?;

        Ok(())
    }

    /// List all file objects in S3 under the files/ prefix
    pub async fn list_objects(&self) -> Result<Vec<S3ObjectInfo>, S3Error> {
        let prefix = self.key("files/");
        let mut objects = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix);

            if let Some(token) = continuation_token.take() {
                request = request.continuation_token(token);
            }

            let output = request.send().await.map_err(|e| S3Error::Read {
                key: prefix.clone(),
                message: e.to_string(),
            })?;

            if let Some(contents) = output.contents {
                for obj in contents {
                    if let (Some(key), Some(etag), Some(size)) =
                        (obj.key.as_ref(), obj.e_tag.as_ref(), obj.size)
                    {
                        // Convert S3 key to VFS path
                        let files_prefix = self.key("files");
                        let path = key.strip_prefix(&files_prefix).unwrap_or(key).to_string();

                        // Extract last_modified timestamp
                        let last_modified = obj.last_modified.map(|t| t.secs() as u64).unwrap_or(0);

                        objects.push(S3ObjectInfo {
                            path,
                            etag: etag.trim_matches('"').to_string(),
                            last_modified,
                            size: size as u64,
                        });
                    }
                }
            }

            if output.is_truncated.unwrap_or(false) {
                continuation_token = output.next_continuation_token;
            } else {
                break;
            }
        }

        Ok(objects)
    }

    /// Get a single file from S3
    ///
    /// Returns (content, etag, last_modified) or None if not found
    pub async fn get_file(&self, path: &str) -> Result<Option<(Vec<u8>, String, u64)>, S3Error> {
        let key = self.key(&format!("files{}", path));

        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(output) => {
                let etag = output
                    .e_tag
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
                let last_modified = output.last_modified.map(|t| t.secs() as u64).unwrap_or(0);

                let data = output.body.collect().await.map_err(|e| S3Error::Read {
                    key: key.clone(),
                    message: e.to_string(),
                })?;

                Ok(Some((data.into_bytes().to_vec(), etag, last_modified)))
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                if error_str.contains("NoSuchKey") || error_str.contains("404") {
                    Ok(None)
                } else {
                    Err(S3Error::Read {
                        key,
                        message: error_str,
                    })
                }
            }
        }
    }

    /// Upload a file and return the ETag
    /// Uses multipart upload for files >= 5MB
    pub async fn put_file_with_etag(&self, path: &str, data: Vec<u8>) -> Result<String, S3Error> {
        let key = self.key(&format!("files{}", path));

        if data.len() >= MULTIPART_THRESHOLD {
            self.multipart_upload(&key, data).await
        } else {
            self.simple_upload(&key, data).await
        }
    }

    /// Simple single-request upload for small files
    async fn simple_upload(&self, key: &str, data: Vec<u8>) -> Result<String, S3Error> {
        let output = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into())
            .send()
            .await
            .map_err(|e| S3Error::Write {
                key: key.to_string(),
                message: e.to_string(),
            })?;

        Ok(output
            .e_tag
            .unwrap_or_default()
            .trim_matches('"')
            .to_string())
    }

    /// Multipart upload for large files with parallel part uploads
    async fn multipart_upload(&self, key: &str, data: Vec<u8>) -> Result<String, S3Error> {
        let total_parts = data.len().div_ceil(PART_SIZE);
        log::info!(
            "[s3] Starting parallel multipart upload for {} ({} bytes, {} parts)",
            key,
            data.len(),
            total_parts
        );

        // 1. Create multipart upload
        let create_output = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| S3Error::Write {
                key: key.to_string(),
                message: format!("Failed to create multipart upload: {}", e),
            })?;

        let upload_id = create_output
            .upload_id()
            .ok_or_else(|| S3Error::Write {
                key: key.to_string(),
                message: "No upload_id returned".to_string(),
            })?
            .to_string();

        // 2. Upload parts in parallel
        let upload_futures: Vec<_> = data
            .chunks(PART_SIZE)
            .enumerate()
            .map(|(i, chunk)| {
                let part_number = (i + 1) as i32;
                let chunk_data = chunk.to_vec();
                let bucket = self.bucket.clone();
                let key = key.to_string();
                let upload_id = upload_id.clone();
                let client = self.client.clone();

                async move {
                    let output = client
                        .upload_part()
                        .bucket(&bucket)
                        .key(&key)
                        .upload_id(&upload_id)
                        .part_number(part_number)
                        .body(chunk_data.into())
                        .send()
                        .await
                        .map_err(|e| S3Error::Write {
                            key: key.clone(),
                            message: format!("Failed to upload part {}: {}", part_number, e),
                        })?;

                    Ok::<_, S3Error>(
                        CompletedPart::builder()
                            .part_number(part_number)
                            .e_tag(output.e_tag().unwrap_or_default())
                            .build(),
                    )
                }
            })
            .collect();

        // Execute all uploads concurrently
        let results = futures::future::join_all(upload_futures).await;

        // Collect results and check for errors
        let mut parts: Vec<CompletedPart> = Vec::with_capacity(total_parts);
        for result in results {
            match result {
                Ok(part) => parts.push(part),
                Err(e) => {
                    // TODO: abort_multipart_upload on error
                    log::error!("[s3] Part upload failed: {}", e);
                    return Err(e);
                }
            }
        }

        // Sort parts by part number (required by S3)
        parts.sort_by_key(|p| p.part_number().unwrap_or(0));

        log::info!(
            "[s3] All {} parts uploaded, completing multipart upload",
            total_parts
        );

        // 3. Complete multipart upload
        let completed = CompletedMultipartUpload::builder()
            .set_parts(Some(parts))
            .build();

        let complete_output = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(completed)
            .send()
            .await
            .map_err(|e| S3Error::Write {
                key: key.to_string(),
                message: format!("Failed to complete multipart upload: {}", e),
            })?;

        log::info!("[s3] Completed multipart upload for {}", key);

        Ok(complete_output
            .e_tag()
            .unwrap_or_default()
            .trim_matches('"')
            .to_string())
    }
}

/// S3 operation errors
#[derive(Debug)]
pub enum S3Error {
    Read { key: String, message: String },
    Write { key: String, message: String },
    Delete { key: String, message: String },
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3Error::Read { key, message } => write!(f, "S3 read error for {}: {}", key, message),
            S3Error::Write { key, message } => write!(f, "S3 write error for {}: {}", key, message),
            S3Error::Delete { key, message } => {
                write!(f, "S3 delete error for {}: {}", key, message)
            }
        }
    }
}

impl std::error::Error for S3Error {}
