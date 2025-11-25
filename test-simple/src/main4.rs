use std::fs;

fn main() {
    // First create a file to ensure the directory is not empty
    if fs::write("/test.txt", b"hello").is_err() {
        std::process::exit(10);
    }

    // Now try to read the directory
    match fs::read_dir("/") {
        Ok(entries) => {
            let mut count = 0;
            for _entry in entries {
                count += 1;
            }

            // We should have at least one entry (test.txt)
            if count > 0 {
                std::process::exit(0);
            } else {
                // Directory is empty - unexpected
                std::process::exit(2);
            }
        }
        Err(_) => {
            // Failed to read root
            std::process::exit(1);
        }
    }
}
