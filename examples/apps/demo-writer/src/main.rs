use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: demo-writer <path> <content>");
        return;
    }

    let path = &args[1];
    let content = &args[2];

    match fs::write(path, content) {
        Ok(()) => println!("Wrote {} bytes to {}", content.len(), path),
        Err(e) => {
            eprintln!("Failed to write to {}: {}", path, e);
            return;
        }
    }
}
