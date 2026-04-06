//! Concurrent Append Client
//!
//! Connects to VFS RPC Server and appends lines to a shared file.
//! Each instance appends multiple lines with a unique client ID.
//!
//! Usage:
//!   CLIENT_ID=1 APPEND_COUNT=100 wasmtime run ... append-client.wasm

use std::fs::OpenOptions;
use std::io::Write;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let client_id: u32 = std::env::var("CLIENT_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let append_count: u32 = std::env::var("APPEND_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let shared_file = "/shared/concurrent.log";

    // Create directory if needed (first client to run)
    let _ = std::fs::create_dir_all("/shared");

    eprintln!("[Client {}] Starting {} appends to {}", client_id, append_count, shared_file);

    let mut success_count = 0;
    let mut error_count = 0;

    for i in 0..append_count {
        // Open file in append mode for each write (simulates real-world usage)
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(shared_file)
        {
            Ok(mut file) => {
                // Write a line with timestamp, client ID and sequence number
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let line = format!("[{}] CLIENT_{:03}:SEQ_{:05}\n", timestamp, client_id, i);
                match file.write_all(line.as_bytes()) {
                    Ok(_) => {
                        success_count += 1;
                        // Sleep 100ms after each write to demonstrate interleaving
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        eprintln!("[Client {}] Write error: {}", client_id, e);
                        error_count += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("[Client {}] Open error: {}", client_id, e);
                error_count += 1;
            }
        }
    }

    eprintln!(
        "[Client {}] Completed: {} success, {} errors",
        client_id, success_count, error_count
    );

    // Print result for verification
    println!("RESULT:client={},success={},errors={}", client_id, success_count, error_count);
}
