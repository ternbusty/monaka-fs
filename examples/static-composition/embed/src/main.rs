use std::fs;

fn main() {
    println!("=== Embedded File Read Test ===");

    // List /data directory
    println!("\nListing /data:");
    match fs::read_dir("/data") {
        Ok(entries) => {
            for entry in entries {
                if let Ok(entry) = entry {
                    println!("  {}", entry.path().display());
                }
            }
        }
        Err(e) => {
            eprintln!("  Failed to list /data: {}", e);
            return;
        }
    }

    // Read each file
    for name in &["hello.txt", "world.txt"] {
        let path = format!("/data/{}", name);
        println!("\nReading {}:", path);
        match fs::read_to_string(&path) {
            Ok(content) => println!("  \"{}\"", content.trim()),
            Err(e) => eprintln!("  Failed: {}", e),
        }
    }

    println!("\n=== Done ===");
}
