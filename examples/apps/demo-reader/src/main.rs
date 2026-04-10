use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: demo-reader <path>");
        return;
    }

    let path = &args[1];

    match fs::read_to_string(path) {
        Ok(content) => print!("{}", content),
        Err(e) => {
            eprintln!("Failed to read {}: {}", path, e);
            return;
        }
    }
}
