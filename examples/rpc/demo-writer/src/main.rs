use std::fs;

fn main() {
    println!("=== VFS Demo App 1: File Writer ===");

    // Write file
    let message = "Hello from App1!";
    println!("\nWriting file: /message.txt");
    match fs::write("/message.txt", message) {
        Ok(()) => println!("  Wrote {} bytes", message.len()),
        Err(e) => {
            eprintln!("  Failed to write: {}", e);
            return;
        }
    }

    println!("\n=== App1 completed successfully ===");
}
