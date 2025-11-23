use fs_core::TimeProvider;
use std::time::{SystemTime, UNIX_EPOCH};

/// Time provider using std::time::SystemTime
pub struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}
