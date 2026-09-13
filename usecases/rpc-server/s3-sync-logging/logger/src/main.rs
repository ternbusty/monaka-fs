//! Logger App - S3 Sync Demo
//!
//! Writes log entries to a shared log file with timestamps.
//! Multiple replicas can run concurrently, each identified by REPLICA_ID.
//! `ENTRY_COUNT` and `ENTRY_DELAY_MS` override the number of entries and
//! the pause between them (the e2e suite uses short runs).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Format timestamp as ISO 8601 (e.g., "2026-01-03T12:34:56.789Z")
fn format_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let total_secs = now.as_secs();
    let millis = now.subsec_millis();

    let days = total_secs / 86400;
    let remaining = total_secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let mut remaining_days = days as i64;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let days_in_months: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days_in_month in days_in_months.iter() {
        if remaining_days < *days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        month += 1;
    }

    (year as u64, month, (remaining_days + 1) as u64)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Append one line, reporting the error kind on failure so a lock
/// timeout (`ResourceBusy`) is visible in the output.
fn append_line(replica_id: &str, log_path: &str, entry: &str) {
    let file = OpenOptions::new().create(true).append(true).open(log_path);
    let mut file = match file {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "[replica-{}] Failed to open log file: {} ({:?})",
                replica_id,
                e,
                e.kind()
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = file.write_all(entry.as_bytes()) {
        eprintln!("[replica-{}] Failed to write log: {}", replica_id, e);
        std::process::exit(1);
    }
    if let Err(e) = file.sync_all() {
        eprintln!(
            "[replica-{}] Failed to sync log file: {} ({:?})",
            replica_id,
            e,
            e.kind()
        );
        std::process::exit(1);
    }
}

fn main() {
    let replica_id = std::env::var("REPLICA_ID").unwrap_or_else(|_| "1".to_string());
    let entry_count = env_u64("ENTRY_COUNT", 10);
    let entry_delay = Duration::from_millis(env_u64("ENTRY_DELAY_MS", 1000));

    println!("[replica-{}] Starting logger...", replica_id);

    // Ensure /logs directory exists
    let _ = fs::create_dir("/logs");

    let log_path = "/logs/app.log";

    // Write log entries with delay to allow interleaving with other replicas
    for i in 1..=entry_count {
        let timestamp = format_timestamp();
        let entry = format!(
            "{} [replica-{}] Entry {}: Processing request...\n",
            timestamp, replica_id, i
        );
        println!("[replica-{}] Entry {}", replica_id, i);

        // Append to shared log file
        append_line(&replica_id, log_path, &entry);

        // Pause before the next entry so replicas interleave
        thread::sleep(entry_delay);
    }

    let timestamp = format_timestamp();
    let entry = format!(
        "{} [replica-{}] Completed all tasks\n",
        timestamp, replica_id
    );
    append_line(&replica_id, log_path, &entry);

    println!("[replica-{}] Wrote log to {}", replica_id, log_path);
}
