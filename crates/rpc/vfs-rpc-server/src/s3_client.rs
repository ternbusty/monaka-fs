//! S3 Client for WASI environment
//!
//! Provides S3 operations for snapshot storage and file synchronization.

use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_wasm::wasi::WasiHttpClientBuilder;

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
        let http_client = WasiHttpClientBuilder::new().build();
        let sleep = TokioSleep::new();

        let mut config_loader = aws_config::defaults(BehaviorVersion::latest())
            .http_client(http_client)
            .sleep_impl(sleep);

        // Check for custom endpoint (LocalStack, MinIO, etc.)
        if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
            println!("[s3] Using custom endpoint: {}", endpoint);
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

    /// Load snapshot from S3
    ///
    /// Returns None if snapshot doesn't exist
    pub async fn load_snapshot(&self) -> Result<Option<Vec<u8>>, S3Error> {
        let key = self.key("snapshot.json");
        println!("[s3] Loading snapshot from s3://{}/{}", self.bucket, key);

        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(output) => {
                println!("[s3] Snapshot found, reading body...");
                let data = output.body.collect().await.map_err(|e| S3Error::Read {
                    key: key.clone(),
                    message: e.to_string(),
                })?;
                Ok(Some(data.into_bytes().to_vec()))
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                println!("[s3] GetObject error: {}", error_str);

                // Check if it's a NoSuchKey error
                if error_str.contains("NoSuchKey") || error_str.contains("404") {
                    println!("[s3] Snapshot does not exist (NoSuchKey)");
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

    /// Save snapshot to S3
    pub async fn save_snapshot(&self, data: &[u8]) -> Result<(), S3Error> {
        let key = self.key("snapshot.json");

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(data.to_vec().into())
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| S3Error::Write {
                key: key.clone(),
                message: e.to_string(),
            })?;

        Ok(())
    }

    /// Upload a single file to S3
    pub async fn put_file(&self, path: &str, data: Vec<u8>) -> Result<(), S3Error> {
        let key = self.key(&format!("files{}", path));

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(data.into())
            .send()
            .await
            .map_err(|e| S3Error::Write {
                key: key.clone(),
                message: e.to_string(),
            })?;

        Ok(())
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
