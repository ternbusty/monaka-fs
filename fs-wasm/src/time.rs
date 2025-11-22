use fs_core::TimeProvider;

pub struct WasiTimeProvider;

impl TimeProvider for WasiTimeProvider {
    fn now(&self) -> u64 {
        #[cfg(target_family = "wasm")]
        unsafe {
            // Use WASI REALTIME clock (nanoseconds, converted to seconds)
            wasi::clock_time_get(wasi::CLOCKID_REALTIME, 1_000_000_000)
                .map(|time| (time / 1_000_000_000) as u64)
                .unwrap_or(0)
        }

        #[cfg(not(target_family = "wasm"))]
        {
            // Non-WASM fallback for testing
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        }
    }
}
