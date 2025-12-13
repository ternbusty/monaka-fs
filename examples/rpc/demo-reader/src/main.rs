//! VFS Demo App 2 - File Reader
//! Uses std::fs transparently over RPC

use std::fs;

fn main() {
    println!("=== VFS Demo App 2: File Reader ===");

    // Read file metadata
    println!("\nGetting file metadata: /shared/message.txt");
    match fs::metadata("/shared/message.txt") {
        Ok(meta) => {
            println!("  File size: {} bytes", meta.len());
        }
        Err(e) => {
            eprintln!("  Failed to get metadata: {}", e);
            eprintln!("  Make sure App1 has been run first!");
            return;
        }
    }

    // Read file content
    println!("\nReading file: /shared/message.txt");
    match fs::read_to_string("/shared/message.txt") {
        Ok(content) => {
            println!("  Content ({} bytes):", content.len());
            println!("  \"{}\"", content);
        }
        Err(e) => {
            eprintln!("  Failed to read: {}", e);
            return;
        }
    }

    println!("\n=== App2 completed ===");
}
