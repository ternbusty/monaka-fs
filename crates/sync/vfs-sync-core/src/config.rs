//! Sync configuration types

use std::time::Duration;

/// Sync operation for outbound queue
#[derive(Debug, Clone)]
pub enum SyncOperation {
    Upload { path: String },
    Delete { path: String },
}

/// Outbound sync mode (writes to S3)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    /// Batch mode: sync every N seconds or N operations (default)
    #[default]
    Batch,
    /// Real-time mode: sync immediately after each write operation
    RealTime,
}

impl SyncMode {
    /// Parse sync mode from environment variable VFS_SYNC_MODE
    pub fn from_env() -> Self {
        match std::env::var("VFS_SYNC_MODE").as_deref() {
            Ok("realtime") | Ok("real-time") | Ok("immediate") => SyncMode::RealTime,
            _ => SyncMode::Batch,
        }
    }
}

/// Inbound sync mode (reads from S3)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InboundMode {
    /// Do not sync from S3 (write-only sync)
    None,
    /// Poll S3 periodically and sync changes (default)
    #[default]
    Polling,
    /// Fetch from S3 on file read (read-through cache)
    ReadThrough,
}

impl InboundMode {
    /// Parse inbound mode from environment variable VFS_INBOUND_MODE
    pub fn from_env() -> Self {
        match std::env::var("VFS_INBOUND_MODE").as_deref() {
            Ok("none") | Ok("disabled") => InboundMode::None,
            Ok("readthrough") | Ok("read-through") => InboundMode::ReadThrough,
            _ => InboundMode::Polling,
        }
    }
}

/// Metadata sync mode (open operations)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataMode {
    /// Use local metadata only (default)
    #[default]
    Local,
    /// Check S3 metadata on open (HEAD request)
    S3,
}

impl MetadataMode {
    /// Parse metadata mode from environment variable VFS_METADATA_MODE
    pub fn from_env() -> Self {
        match std::env::var("VFS_METADATA_MODE").as_deref() {
            Ok("s3") => MetadataMode::S3,
            _ => MetadataMode::Local,
        }
    }
}

/// Sync manager configuration
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Outbound sync mode (batch or realtime)
    pub mode: SyncMode,
    /// Inbound sync mode (none, polling, or read-through)
    pub inbound_mode: InboundMode,
    /// Metadata sync mode (local or S3)
    pub metadata_mode: MetadataMode,
    /// Interval for S3 polling (inbound, when using Polling mode)
    pub poll_interval: Duration,
    /// Interval for outbound queue flush (batch mode only)
    pub flush_interval: Duration,
    /// Maximum operations per outbound flush (batch mode only)
    pub outbound_batch_size: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            mode: SyncMode::from_env(),
            inbound_mode: InboundMode::from_env(),
            metadata_mode: MetadataMode::from_env(),
            poll_interval: Duration::from_secs(30),
            flush_interval: Duration::from_secs(5),
            outbound_batch_size: 10,
        }
    }
}

impl SyncConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self::default()
    }
}
