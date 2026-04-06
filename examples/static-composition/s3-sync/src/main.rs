//! Static Composition + S3 Sync Demo
//!
//! This example demonstrates using VFS with S3 synchronization
//! through static composition (wac plug).
//!
//! Files written to the VFS will be automatically synced to S3
//! based on the sync configuration (batch or realtime mode).

use std::fs;
use std::io::Write;

fn main() {
    println!("=== Static Composition + S3 Sync Demo ===\n");

    // Create a directory for our data
    println!("1. Creating /data directory...");
    if let Err(e) = fs::create_dir("/data") {
        // Directory might already exist
        println!("   Note: {}", e);
    } else {
        println!("   Created /data");
    }

    // Write some files that will be synced to S3
    println!("\n2. Writing files (will be synced to S3)...");

    let files = [
        ("/data/config.json", r#"{"version": "1.0", "enabled": true}"#),
        ("/data/log.txt", "2024-01-01 12:00:00 - Application started\n"),
        ("/data/metrics.csv", "timestamp,value\n1704067200,42\n1704070800,55\n"),
    ];

    for (path, content) in &files {
        match fs::write(path, content) {
            Ok(_) => println!("   Wrote {} ({} bytes)", path, content.len()),
            Err(e) => println!("   Failed to write {}: {}", path, e),
        }
    }

    // Append to log file (demonstrates multiple writes)
    println!("\n3. Appending to log file...");
    match fs::OpenOptions::new().append(true).open("/data/log.txt") {
        Ok(mut file) => {
            let log_entry = "2024-01-01 12:01:00 - Processing data\n";
            if let Err(e) = file.write_all(log_entry.as_bytes()) {
                println!("   Failed to append: {}", e);
            } else {
                println!("   Appended log entry");
            }
        }
        Err(e) => println!("   Failed to open log: {}", e),
    }

    // Read back and verify
    println!("\n4. Verifying written data...");
    for (path, _) in &files {
        match fs::read_to_string(path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                println!("   {} - {} lines", path, lines.len());
            }
            Err(e) => println!("   Failed to read {}: {}", path, e),
        }
    }

    // List directory contents
    println!("\n5. Directory listing /data:");
    match fs::read_dir("/data") {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                let metadata = entry.metadata().ok();
                let size = metadata.map(|m| m.len()).unwrap_or(0);
                println!("   {} ({} bytes)", path.display(), size);
            }
        }
        Err(e) => println!("   Failed to list directory: {}", e),
    }

    // Create nested structure
    println!("\n6. Creating nested directory structure...");
    if let Err(e) = fs::create_dir_all("/data/archive/2024/01") {
        println!("   Failed: {}", e);
    } else {
        println!("   Created /data/archive/2024/01");

        // Write archived file
        let archive_path = "/data/archive/2024/01/backup.txt";
        if let Err(e) = fs::write(archive_path, "Archived data from January 2024") {
            println!("   Failed to write archive: {}", e);
        } else {
            println!("   Wrote {}", archive_path);
        }
    }

    println!("\n=== Demo Complete ===");
    println!("\nFiles written to VFS will be synced to S3 based on:");
    println!("  - VFS_S3_SYNC_MODE: 'batch' (default) or 'realtime'");
    println!("  - VFS_S3_FLUSH_INTERVAL: seconds between batch flushes");
    println!("\nCheck your S3 bucket to verify the sync.");
}
