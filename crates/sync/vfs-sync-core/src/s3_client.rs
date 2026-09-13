//! S3 client for VFS persistence (host- and WASI-shared).
//!
//! Implements [`ObjectStore`] over the AWS SDK. The `new()` constructor is
//! environment-specific (different HTTP clients), so consumers build an
//! `aws_config::SdkConfig` themselves and call [`S3Storage::from_sdk_config`].

use std::sync::atomic::{AtomicBool, Ordering};

use aws_config::SdkConfig;
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;

use crate::object_store::ObjectStore;
use crate::types::{ObjectMeta, Precondition, S3Error};

/// Multipart upload threshold (10MB)
const MULTIPART_THRESHOLD: usize = 10 * 1024 * 1024;
/// Part size for multipart upload (10MB)
const PART_SIZE: usize = 10 * 1024 * 1024;

/// S3 client wrapper for VFS persistence.
pub struct S3Storage {
    client: Client,
    bucket: String,
    prefix: String,
    /// Cleared the first time the backend answers a conditional
    /// `DeleteObject` with `NotImplemented` (LocalStack 4.x). After that,
    /// deletes fall back to a HEAD comparison followed by an unconditional
    /// delete.
    conditional_delete_supported: AtomicBool,
}

#[derive(Clone, Copy)]
enum Kind {
    Read,
    Write,
    Delete,
}

fn quote_etag(etag: &str) -> String {
    format!("\"{}\"", etag.trim_matches('"'))
}

fn unquote(etag: Option<String>) -> String {
    etag.unwrap_or_default().trim_matches('"').to_string()
}

/// Map an SDK error to [`S3Error`], preserving the HTTP status and error
/// code for the cases the sync logic branches on.
fn classify<E>(key: &str, e: SdkError<E, HttpResponse>, kind: Kind) -> S3Error
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
{
    let status = e.raw_response().map(|r| r.status().as_u16());
    let code = e.code().unwrap_or("").to_string();
    let message = match e.as_service_error() {
        Some(se) => format!("{} ({})", se, code),
        None => e.to_string(),
    };
    let key = key.to_string();

    match (status, code.as_str()) {
        (Some(412), _)
        | (Some(409), _)
        | (_, "PreconditionFailed")
        | (_, "ConditionalRequestConflict") => S3Error::PreconditionFailed {
            key,
            status: status.unwrap_or(0),
            code,
        },
        (Some(404), _) | (_, "NotFound") | (_, "NoSuchKey") => S3Error::NotFound { key },
        (Some(501), _) | (_, "NotImplemented") => S3Error::NotImplemented { key, message },
        _ => match kind {
            Kind::Read => S3Error::Read { key, message },
            Kind::Write => S3Error::Write { key, message },
            Kind::Delete => S3Error::Delete { key, message },
        },
    }
}

impl S3Storage {
    /// Build an `S3Storage` from a pre-configured `SdkConfig`. Consumers in
    /// `vfs-sync-host` and `vfs-sync-adapter` set up their environment-
    /// specific HTTP client (hyper / WASI-HTTP) before calling this.
    ///
    /// Path-style addressing is forced for LocalStack/MinIO compatibility.
    pub fn from_sdk_config(bucket: String, prefix: String, config: &SdkConfig) -> Self {
        let s3_config = aws_sdk_s3::config::Builder::from(config)
            .force_path_style(true)
            .build();
        let client = Client::from_conf(s3_config);
        Self {
            client,
            bucket,
            prefix,
            conditional_delete_supported: AtomicBool::new(true),
        }
    }

    /// Full S3 key for a prefix-relative key.
    fn key(&self, rel: &str) -> String {
        format!("{}{}", self.prefix, rel.trim_start_matches('/'))
    }

    /// Prefix-relative key for a full S3 key.
    fn rel(&self, full: &str) -> String {
        full.strip_prefix(&self.prefix).unwrap_or(full).to_string()
    }

    async fn simple_upload(
        &self,
        key: &str,
        data: Vec<u8>,
        cond: &Precondition,
    ) -> Result<String, S3Error> {
        let mut req = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into());
        req = match cond {
            Precondition::None => req,
            Precondition::IfMatch(etag) => req.if_match(quote_etag(etag)),
            Precondition::IfNoneMatchAny => req.if_none_match("*"),
        };
        let output = req
            .send()
            .await
            .map_err(|e| classify(key, e, Kind::Write))?;
        Ok(unquote(output.e_tag))
    }

    async fn multipart_upload(
        &self,
        key: &str,
        data: Vec<u8>,
        cond: &Precondition,
    ) -> Result<String, S3Error> {
        let total_parts = data.len().div_ceil(PART_SIZE);
        log::info!(
            "[s3] Starting parallel multipart upload for {} ({} bytes, {} parts)",
            key,
            data.len(),
            total_parts
        );

        let create_output = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| classify(key, e, Kind::Write))?;

        let upload_id = create_output
            .upload_id()
            .ok_or_else(|| S3Error::Write {
                key: key.to_string(),
                message: "No upload_id returned".to_string(),
            })?
            .to_string();

        match self
            .multipart_upload_inner(key, data, &upload_id, total_parts, cond)
            .await
        {
            Ok(etag) => Ok(etag),
            Err(e) => {
                log::error!(
                    "[s3] Multipart upload failed for {}: {}. Aborting upload {}.",
                    key,
                    e,
                    upload_id
                );
                if let Err(abort_err) = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.bucket)
                    .key(key)
                    .upload_id(&upload_id)
                    .send()
                    .await
                {
                    log::error!(
                        "[s3] Failed to abort multipart upload {} for {}: {}",
                        upload_id,
                        key,
                        abort_err
                    );
                }
                Err(e)
            }
        }
    }

    async fn multipart_upload_inner(
        &self,
        key: &str,
        data: Vec<u8>,
        upload_id: &str,
        total_parts: usize,
        cond: &Precondition,
    ) -> Result<String, S3Error> {
        let upload_futures: Vec<_> = data
            .chunks(PART_SIZE)
            .enumerate()
            .map(|(i, chunk)| {
                let part_number = (i + 1) as i32;
                let chunk_data = chunk.to_vec();
                let bucket = self.bucket.clone();
                let key = key.to_string();
                let upload_id = upload_id.to_string();
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
                        .map_err(|e| classify(&key, e, Kind::Write))?;

                    Ok::<_, S3Error>(
                        CompletedPart::builder()
                            .part_number(part_number)
                            .e_tag(output.e_tag().unwrap_or_default())
                            .build(),
                    )
                }
            })
            .collect();

        let results = futures::future::join_all(upload_futures).await;

        let mut parts: Vec<CompletedPart> = Vec::with_capacity(total_parts);
        for result in results {
            parts.push(result?);
        }

        parts.sort_by_key(|p| p.part_number().unwrap_or(0));

        log::info!(
            "[s3] All {} parts uploaded, completing multipart upload",
            total_parts
        );

        let completed = CompletedMultipartUpload::builder()
            .set_parts(Some(parts))
            .build();

        let mut req = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(completed);
        req = match cond {
            Precondition::None => req,
            Precondition::IfMatch(etag) => req.if_match(quote_etag(etag)),
            Precondition::IfNoneMatchAny => req.if_none_match("*"),
        };
        let complete_output = req
            .send()
            .await
            .map_err(|e| classify(key, e, Kind::Write))?;

        log::info!("[s3] Completed multipart upload for {}", key);

        Ok(unquote(complete_output.e_tag().map(|s| s.to_string())))
    }

    /// Emulate `If-Match` on delete for backends that reject the header:
    /// compare the current ETag first, then delete unconditionally. Not
    /// atomic, but the best available on such backends.
    async fn delete_with_head_check(&self, key: &str, expected: &str) -> Result<(), S3Error> {
        let rel = self.rel(key);
        match self.head(&rel).await? {
            None => Ok(()),
            Some(meta) if meta.etag == expected => {
                self.client
                    .delete_object()
                    .bucket(&self.bucket)
                    .key(key)
                    .send()
                    .await
                    .map_err(|e| classify(key, e, Kind::Delete))?;
                Ok(())
            }
            Some(_) => Err(S3Error::PreconditionFailed {
                key: key.to_string(),
                status: 412,
                code: "PreconditionFailed".into(),
            }),
        }
    }
}

impl ObjectStore for S3Storage {
    async fn list(
        &self,
        prefix: &str,
        max_keys: Option<usize>,
    ) -> Result<Vec<ObjectMeta>, S3Error> {
        let full_prefix = self.key(prefix);
        let mut objects = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&full_prefix);
            if let Some(n) = max_keys {
                request = request.max_keys(n as i32);
            }
            if let Some(token) = continuation_token.take() {
                request = request.continuation_token(token);
            }

            let output = request
                .send()
                .await
                .map_err(|e| classify(&full_prefix, e, Kind::Read))?;

            if let Some(contents) = output.contents {
                for obj in contents {
                    if let (Some(key), Some(etag)) = (obj.key.as_ref(), obj.e_tag.as_ref()) {
                        objects.push(ObjectMeta {
                            key: self.rel(key),
                            etag: etag.trim_matches('"').to_string(),
                            last_modified: obj.last_modified.map(|t| t.secs() as u64).unwrap_or(0),
                            size: obj.size.unwrap_or(0) as u64,
                        });
                    }
                }
            }

            if let Some(n) = max_keys {
                objects.truncate(n);
                break;
            }
            if output.is_truncated.unwrap_or(false) {
                continuation_token = output.next_continuation_token;
            } else {
                break;
            }
        }

        Ok(objects)
    }

    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>, S3Error> {
        let full = self.key(key);
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&full)
            .send()
            .await
        {
            Ok(output) => Ok(Some(ObjectMeta {
                key: key.to_string(),
                etag: unquote(output.e_tag),
                last_modified: output.last_modified.map(|t| t.secs() as u64).unwrap_or(0),
                size: output.content_length.unwrap_or(0) as u64,
            })),
            Err(e) => match classify(&full, e, Kind::Read) {
                S3Error::NotFound { .. } => Ok(None),
                other => Err(other),
            },
        }
    }

    async fn get(&self, key: &str) -> Result<Option<(Vec<u8>, ObjectMeta)>, S3Error> {
        let full = self.key(key);
        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&full)
            .send()
            .await
        {
            Ok(output) => {
                let etag = unquote(output.e_tag);
                let last_modified = output.last_modified.map(|t| t.secs() as u64).unwrap_or(0);
                let data = output.body.collect().await.map_err(|e| S3Error::Read {
                    key: full.clone(),
                    message: e.to_string(),
                })?;
                let bytes = data.into_bytes().to_vec();
                let size = bytes.len() as u64;
                Ok(Some((
                    bytes,
                    ObjectMeta {
                        key: key.to_string(),
                        etag,
                        last_modified,
                        size,
                    },
                )))
            }
            Err(e) => match classify(&full, e, Kind::Read) {
                S3Error::NotFound { .. } => Ok(None),
                other => Err(other),
            },
        }
    }

    async fn put(&self, key: &str, body: Vec<u8>, cond: Precondition) -> Result<String, S3Error> {
        let full = self.key(key);
        if body.len() >= MULTIPART_THRESHOLD {
            self.multipart_upload(&full, body, &cond).await
        } else {
            self.simple_upload(&full, body, &cond).await
        }
    }

    async fn delete(&self, key: &str, cond: Precondition) -> Result<(), S3Error> {
        let full = self.key(key);

        if let Precondition::IfMatch(expected) = &cond {
            if !self.conditional_delete_supported.load(Ordering::Relaxed) {
                return self.delete_with_head_check(&full, expected).await;
            }
        }

        let mut req = self.client.delete_object().bucket(&self.bucket).key(&full);
        if let Precondition::IfMatch(etag) = &cond {
            req = req.if_match(quote_etag(etag));
        }

        match req.send().await {
            Ok(_) => Ok(()),
            Err(e) => match classify(&full, e, Kind::Delete) {
                S3Error::NotFound { .. } => Ok(()),
                S3Error::NotImplemented { .. } if matches!(cond, Precondition::IfMatch(_)) => {
                    log::warn!(
                        "[s3] Backend does not implement conditional DeleteObject; falling back to HEAD comparison"
                    );
                    self.conditional_delete_supported
                        .store(false, Ordering::Relaxed);
                    let Precondition::IfMatch(expected) = &cond else {
                        unreachable!()
                    };
                    self.delete_with_head_check(&full, expected).await
                }
                other => Err(other),
            },
        }
    }
}
