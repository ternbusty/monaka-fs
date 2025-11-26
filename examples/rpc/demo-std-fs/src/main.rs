//! Demo application using std::fs transparently over RPC
//!
//! This app demonstrates that standard Rust filesystem operations
//! work seamlessly when routed through vfs-rpc-host.

use std::fs;

fn main() {
    println!("=== Demo: std::fs over RPC ===");
    println!();

    // Test 1: Write a file
    println!("Test 1: Writing file with std::fs::write");
    let message = "Hello from std::fs over RPC!";
    match fs::write("test.txt", message) {
        Ok(()) => println!("  ✓ Successfully wrote {} bytes", message.len()),
        Err(e) => {
            eprintln!("  ✗ Failed to write: {}", e);
            return;
        }
    }
    println!();

    // Test 2: Read the file back
    println!("Test 2: Reading file with std::fs::read_to_string");
    match fs::read_to_string("test.txt") {
        Ok(content) => {
            println!("  ✓ Successfully read file");
            println!("  Content: \"{}\"", content);
        }
        Err(e) => {
            eprintln!("  ✗ Failed to read: {}", e);
            return;
        }
    }
    println!();

    // Test 3: Get file metadata
    println!("Test 3: Getting metadata with std::fs::metadata");
    match fs::metadata("test.txt") {
        Ok(metadata) => {
            println!("  ✓ File size: {} bytes", metadata.len());
            println!("  ✓ Is file: {}", metadata.is_file());
            println!("  ✓ Is directory: {}", metadata.is_dir());
        }
        Err(e) => {
            eprintln!("  ✗ Failed to get metadata: {}", e);
            return;
        }
    }
    println!();

    // Test 4: Create a directory
    println!("Test 4: Creating directory with std::fs::create_dir");
    match fs::create_dir("test_dir") {
        Ok(()) => println!("  ✓ Directory created"),
        Err(e) => {
            eprintln!("  ✗ Failed to create directory: {}", e);
            // Don't return - directory might already exist
        }
    }
    println!();

    // Test 5: Write a file in the directory
    println!("Test 5: Writing file in subdirectory");
    match fs::write("test_dir/nested.txt", "Nested file content") {
        Ok(()) => println!("  ✓ Nested file created"),
        Err(e) => {
            eprintln!("  ✗ Failed to write nested file: {}", e);
            return;
        }
    }
    println!();

    // Test 6: Read nested file
    println!("Test 6: Reading nested file");
    match fs::read_to_string("test_dir/nested.txt") {
        Ok(content) => {
            println!("  ✓ Nested file content: \"{}\"", content);
        }
        Err(e) => {
            eprintln!("  ✗ Failed to read nested file: {}", e);
            return;
        }
    }
    println!();

    // Test 7: Remove file
    println!("Test 7: Removing file with std::fs::remove_file");
    match fs::remove_file("test.txt") {
        Ok(()) => println!("  ✓ File removed"),
        Err(e) => {
            eprintln!("  ✗ Failed to remove file: {}", e);
            return;
        }
    }
    println!();

    println!("=== All tests completed successfully! ===");
}
