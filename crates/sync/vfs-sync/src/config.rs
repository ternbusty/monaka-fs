//! Sync configuration types

use std::time::Duration;

/// Sync mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    /// Batch mode: sync every N seconds or N operations (default)
    #[default]
    Batch,
    /// Real-time mode: sync immediately after each write operation
    RealTime,
}

impl SyncMode {
    /// Parse sync mode from environment variable
    pub fn from_env() -> Self {
        match std::env::var("VFS_SYNC_MODE").as_deref() {
            Ok("realtime") | Ok("real-time") | Ok("immediate") => SyncMode::RealTime,
            _ => SyncMode::Batch,
        }
    }
}

/// Sync manager configuration
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Sync mode (batch or realtime)
    pub mode: SyncMode,
    /// Interval for S3 polling (inbound)
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
