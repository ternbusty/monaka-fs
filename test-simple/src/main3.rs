use std::fs;

fn main() {
    // Test 1: Just list the root directory
    match fs::read_dir("/") {
        Ok(_entries) => {
            // Success - can read root
            std::process::exit(0);
        }
        Err(_) => {
            // Failed to read root
            std::process::exit(1);
        }
    }
}
