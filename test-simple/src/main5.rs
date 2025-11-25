use std::fs;
use std::io::Write;

fn main() {
    // Test 1: Create a file
    match fs::File::create("/test.txt") {
        Ok(mut f) => {
            // File created successfully
            if f.write_all(b"hello").is_err() {
                std::process::exit(11); // Write failed
            }
        }
        Err(_) => {
            std::process::exit(10); // Create failed
        }
    }

    // Test 2: Try to open root directory
    match fs::read_dir("/") {
        Ok(entries) => {
            // Successfully got DirectoryEntryStream
            let mut count = 0;
            for entry_result in entries {
                match entry_result {
                    Ok(_entry) => {
                        count += 1;
                    }
                    Err(_) => {
                        std::process::exit(21); // Error iterating entry
                    }
                }
            }

            if count > 0 {
                std::process::exit(0); // Success with entries
            } else {
                std::process::exit(20); // No entries found
            }
        }
        Err(_) => {
            std::process::exit(1); // read_dir failed
        }
    }
}
