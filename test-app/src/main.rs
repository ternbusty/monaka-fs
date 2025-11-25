use std::fs;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Test App - Using std::fs with VFS Adapter");
    println!("==========================================\n");

    // Test 1: Create and write to a file
    println!("Test 1: Writing to file");
    let mut file = fs::File::create("/hello.txt")?;
    file.write_all(b"Hello from test-app!")?;
    println!("  ✓ Created and wrote to /hello.txt");

    // Test 2: Read the file back
    println!("\nTest 2: Reading from file");
    let content = fs::read_to_string("/hello.txt")?;
    println!("  Content: {}", content);
    assert_eq!(content, "Hello from test-app!");
    println!("  ✓ Content verified");

    // Test 3: Create a directory
    println!("\nTest 3: Creating directory");
    fs::create_dir("/data")?;
    println!("  ✓ Created directory /data");

    // Test 4: Write file in directory
    println!("\nTest 4: Writing file in directory");
    fs::write("/data/config.txt", b"key=value")?;
    println!("  ✓ Created /data/config.txt");

    // Test 5: Read file from directory
    println!("\nTest 5: Reading file from directory");
    let data = fs::read_to_string("/data/config.txt")?;
    println!("  Content: {}", data);
    assert_eq!(data, "key=value");
    println!("  ✓ Content verified");

    println!("\n✓ All tests passed!");
    Ok(())
}
