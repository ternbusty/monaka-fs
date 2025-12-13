//! VFS Demo App 1 - File Writer
//! Uses std::fs transparently over RPC

use std::fs;

fn main() {
    println!("=== VFS Demo App 1: File Writer ===");

    // Create /shared directory
    println!("\nCreating directory: /shared");
    match fs::create_dir("/shared") {
        Ok(()) => println!("  Directory created"),
        Err(e) => println!("  Directory creation: {}", e),
    }

    // Write file
    let message = "Hello from App1! This file is shared via VFS RPC.";
    println!("\nWriting file: /shared/message.txt");
    match fs::write("/shared/message.txt", message) {
        Ok(()) => println!("  Wrote {} bytes", message.len()),
        Err(e) => {
            eprintln!("  Failed to write: {}", e);
            return;
        }
    }

    println!("\n=== App1 completed successfully ===");
}
