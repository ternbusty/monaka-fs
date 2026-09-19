//! Sync configuration types

use std::time::Duration;

/// Sync operation for outbound queue
#[derive(Debug, Clone)]
pub enum SyncOperation {
    Upload {
        path: String,
    },
    /// `etag` is the last ETag this instance knew for the object, captured
    /// when the delete was enqueued so the eventual `DeleteObject` can be
    /// conditional on it.
    Delete {
        path: String,
        etag: Option<String>,
    },
}

impl SyncOperation {
    pub fn path(&self) -> &str {
        match self {
            SyncOperation::Upload { path } | SyncOperation::Delete { path, .. } => path,
        }
    }
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
        Self::parse(std::env::var("VFS_SYNC_MODE").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("realtime") | Some("real-time") | Some("immediate") => SyncMode::RealTime,
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
        Self::parse(std::env::var("VFS_INBOUND_MODE").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("none") | Some("disabled") => InboundMode::None,
            Some("readthrough") | Some("read-through") => InboundMode::ReadThrough,
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
        Self::parse(std::env::var("VFS_METADATA_MODE").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("s3") => MetadataMode::S3,
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
    /// Per-file S3 lease locks on open-for-write (`VFS_S3_FILE_LOCK`)
    pub file_lock: bool,
    /// How long to wait for a lease held by another instance before
    /// returning `Busy` (`VFS_S3_FILE_LOCK_TIMEOUT_MS`)
    pub lock_timeout: Duration,
    /// Lease duration. A holder that stops renewing loses the lease after
    /// this long (`VFS_S3_FILE_LOCK_LEASE_SECS`)
    pub lock_lease: Duration,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl SyncConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Create config from an arbitrary key lookup. `from_env` is this with
    /// `std::env::var`; tests pass a closure over a map so they never touch
    /// process-wide environment state.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let get = |key: &str| lookup(key);
        Self {
            mode: SyncMode::parse(get("VFS_SYNC_MODE").as_deref()),
            inbound_mode: InboundMode::parse(get("VFS_INBOUND_MODE").as_deref()),
            metadata_mode: MetadataMode::parse(get("VFS_METADATA_MODE").as_deref()),
            poll_interval: parse_secs(get("VFS_POLL_INTERVAL_SECS"), 30),
            flush_interval: parse_secs(get("VFS_FLUSH_INTERVAL_SECS"), 5),
            outbound_batch_size: parse_usize(get("VFS_OUTBOUND_BATCH_SIZE"), 10),
            file_lock: parse_enabled(get("VFS_S3_FILE_LOCK"), true),
            lock_timeout: parse_millis(get("VFS_S3_FILE_LOCK_TIMEOUT_MS"), 10_000),
            lock_lease: parse_secs(get("VFS_S3_FILE_LOCK_LEASE_SECS"), 30),
        }
    }
}

fn parse_secs(value: Option<String>, default_secs: u64) -> Duration {
    value
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(default_secs))
}

fn parse_millis(value: Option<String>, default_ms: u64) -> Duration {
    value
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

fn parse_usize(value: Option<String>, default: usize) -> usize {
    value
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// Accepts `enabled` / `disabled` (case-insensitive). Anything else,
/// including unset, yields `default`.
fn parse_enabled(value: Option<String>, default: bool) -> bool {
    match value.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
        Some(v) if v == "enabled" => true,
        Some(v) if v == "disabled" => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> SyncConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        SyncConfig::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn file_lock_defaults_to_enabled() {
        let c = cfg(&[]);
        assert!(c.file_lock);
        assert_eq!(c.lock_timeout, Duration::from_millis(10_000));
        assert_eq!(c.lock_lease, Duration::from_secs(30));
    }

    #[test]
    fn file_lock_parses_enabled_and_disabled_case_insensitively() {
        assert!(!cfg(&[("VFS_S3_FILE_LOCK", "disabled")]).file_lock);
        assert!(!cfg(&[("VFS_S3_FILE_LOCK", "Disabled")]).file_lock);
        assert!(cfg(&[("VFS_S3_FILE_LOCK", "ENABLED")]).file_lock);
    }

    #[test]
    fn file_lock_unknown_value_falls_back_to_default() {
        assert!(cfg(&[("VFS_S3_FILE_LOCK", "on")]).file_lock);
        assert!(cfg(&[("VFS_S3_FILE_LOCK", "")]).file_lock);
    }

    #[test]
    fn lock_timeout_and_lease_parse_and_fall_back() {
        let c = cfg(&[
            ("VFS_S3_FILE_LOCK_TIMEOUT_MS", "250"),
            ("VFS_S3_FILE_LOCK_LEASE_SECS", "7"),
        ]);
        assert_eq!(c.lock_timeout, Duration::from_millis(250));
        assert_eq!(c.lock_lease, Duration::from_secs(7));

        let c = cfg(&[
            ("VFS_S3_FILE_LOCK_TIMEOUT_MS", "abc"),
            ("VFS_S3_FILE_LOCK_LEASE_SECS", "-1"),
        ]);
        assert_eq!(c.lock_timeout, Duration::from_millis(10_000));
        assert_eq!(c.lock_lease, Duration::from_secs(30));
    }

    #[test]
    fn existing_modes_still_parse() {
        let c = cfg(&[
            ("VFS_SYNC_MODE", "realtime"),
            ("VFS_INBOUND_MODE", "none"),
            ("VFS_METADATA_MODE", "s3"),
            ("VFS_OUTBOUND_BATCH_SIZE", "3"),
        ]);
        assert_eq!(c.mode, SyncMode::RealTime);
        assert_eq!(c.inbound_mode, InboundMode::None);
        assert_eq!(c.metadata_mode, MetadataMode::S3);
        assert_eq!(c.outbound_batch_size, 3);
    }
}
