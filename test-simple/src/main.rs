use std::fs;
use std::io::Write;

fn main() {
    // Test 1: Create and write to a file
    let mut file = match fs::File::create("/hello.txt") {
        Ok(f) => f,
        Err(_) => std::process::exit(1),
    };

    if file.write_all(b"Hello!").is_err() {
        std::process::exit(2);
    }
    drop(file);

    // Test 2: Read the file back
    let content = match fs::read_to_string("/hello.txt") {
        Ok(c) => c,
        Err(_) => std::process::exit(3),
    };

    if content != "Hello!" {
        std::process::exit(4);
    }

    // Test 3: Create a directory
    if fs::create_dir("/data").is_err() {
        std::process::exit(5);
    }

    // Test 4: Write file in directory
    if fs::write("/data/test.txt", b"test").is_err() {
        std::process::exit(6);
    }

    // Success
    std::process::exit(0);
}
