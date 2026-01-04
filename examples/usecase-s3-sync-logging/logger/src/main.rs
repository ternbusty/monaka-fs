//! Logger App - S3 Sync Demo
//!
//! Writes log entries to a shared log file with timestamps.
//! Multiple replicas can run concurrently, each identified by REPLICA_ID.

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

fn main() {
    let replica_id = std::env::var("REPLICA_ID").unwrap_or_else(|_| "1".to_string());

    println!("[replica-{}] Starting logger...", replica_id);

    // Ensure /logs directory exists
    let _ = fs::create_dir("/logs");

    let log_path = "/logs/app.log";

    // Write log entries with delay to allow interleaving with other replicas
    for i in 1..=10 {
        let timestamp = format_timestamp();
        let entry = format!(
            "{} [replica-{}] Entry {}: Processing request...\n",
            timestamp, replica_id, i
        );
        println!("[replica-{}] Entry {}", replica_id, i);

        // Append to shared log file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("Failed to open log file");
        file.write_all(entry.as_bytes())
            .expect("Failed to write log");

        // Wait 1 second before next entry
        thread::sleep(Duration::from_secs(1));
    }

    let timestamp = format_timestamp();
    let entry = format!(
        "{} [replica-{}] Completed all tasks\n",
        timestamp, replica_id
    );
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("Failed to open log file");
    file.write_all(entry.as_bytes())
        .expect("Failed to write log");

    println!("[replica-{}] Wrote log to {}", replica_id, log_path);
}
