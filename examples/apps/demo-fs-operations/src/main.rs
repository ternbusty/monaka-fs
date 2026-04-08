// Component Model Demo Application
// This application demonstrates file system operations using WASI filesystem API.
// It will be composed with the vfs-provider component to use in-memory VFS.

use std::fs;
use std::io::Write;

fn main() {
    println!("=== Component Model VFS Demo ===\n");

    // Test 1: Basic file operations
    test_basic_file_operations();

    // Test 2: Directory operations
    test_directory_operations();

    // Test 3: Metadata operations
    test_metadata_operations();

    // Test 4: Error handling
    test_error_handling();

    // Test 5: Rename/move operations
    test_rename_operations();

    // Additional test: Comprehensive operation
    test_operation();

    println!("\n=== All tests completed ===");
}

fn test_operation() {
    let directory_path = "/testdir";
    fs::create_dir(directory_path);

    let nested_directory_path = "/testdir/sub1/sub2";
    fs::create_dir_all(nested_directory_path);

    let file_path = "/testdir/test.txt";
    let content = "Hello from Component Model!";
    fs::write(file_path, content);

    let metadata = fs::metadata(file_path);
    println!("File metadata: {:?}", metadata);

    let read_content = fs::read_to_string(file_path);
    println!("Read content: {}", read_content.unwrap());

    fs::read_dir(directory_path).unwrap().for_each(|entry| {
        let entry = entry.unwrap();
        println!("Entry: {}", entry.path().display());
    });

    fs::remove_file(file_path);

    fs::read_dir(directory_path).unwrap().for_each(|entry| {
        let entry = entry.unwrap();
        println!("Entry after file remove: {}", entry.path().display());
    });
}

fn test_basic_file_operations() {
    println!("Test 1: Basic File Operations");
    println!("------------------------------");

    // Create and write to a file
    let filename = "/test.txt";
    let content = "Hello from Component Model!";

    match fs::write(filename, content) {
        Ok(_) => println!("✓ Created and wrote to {}", filename),
        Err(e) => println!("✗ Failed to write file: {}", e),
    }

    // Read the file back
    match fs::read_to_string(filename) {
        Ok(data) => {
            if data == content {
                println!("✓ Read file successfully: '{}'", data);
            } else {
                println!("✗ Content mismatch: expected '{}', got '{}'", content, data);
            }
        }
        Err(e) => println!("✗ Failed to read file: {}", e),
    }

    // Append to the file
    match fs::OpenOptions::new().append(true).open(filename) {
        Ok(mut file) => {
            let append_content = "\nAppended line";
            match file.write_all(append_content.as_bytes()) {
                Ok(_) => println!("✓ Appended to file"),
                Err(e) => println!("✗ Failed to append: {}", e),
            }
        }
        Err(e) => println!("✗ Failed to open file for append: {}", e),
    }

    // Verify appended content
    match fs::read_to_string(filename) {
        Ok(data) => {
            let expected = format!("{}\nAppended line", content);
            if data == expected {
                println!("✓ Append verification successful");
            } else {
                println!("✗ Append verification failed");
            }
        }
        Err(e) => println!("✗ Failed to read appended file: {}", e),
    }

    // Delete the file
    match fs::remove_file(filename) {
        Ok(_) => println!("✓ Deleted {}", filename),
        Err(e) => println!("✗ Failed to delete file: {}", e),
    }

    println!();
}

fn test_directory_operations() {
    println!("Test 2: Directory Operations");
    println!("----------------------------");

    // Create a directory
    let dirname = "/testdir";
    match fs::create_dir(dirname) {
        Ok(_) => println!("✓ Created directory {}", dirname),
        Err(e) => println!("✗ Failed to create directory: {}", e),
    }

    // Create nested directories
    let nested = "/testdir/sub1/sub2";
    match fs::create_dir_all(nested) {
        Ok(_) => println!("✓ Created nested directories {}", nested),
        Err(e) => println!("✗ Failed to create nested directories: {}", e),
    }

    // Create files in the directory
    let files = vec![
        "/testdir/file1.txt",
        "/testdir/file2.txt",
        "/testdir/sub1/file3.txt",
    ];

    for file in &files {
        match fs::write(file, format!("Content of {}", file)) {
            Ok(_) => println!("✓ Created {}", file),
            Err(e) => println!("✗ Failed to create {}: {}", file, e),
        }
    }

    // List directory contents
    match fs::read_dir(dirname) {
        Ok(entries) => {
            println!("✓ Listing contents of {}:", dirname);
            for entry in entries {
                match entry {
                    Ok(e) => {
                        let path = e.path();
                        let file_type = if e.file_type().unwrap().is_dir() {
                            "[DIR]"
                        } else {
                            "[FILE]"
                        };
                        println!("  {} {}", file_type, path.display());
                    }
                    Err(e) => println!("  ✗ Error reading entry: {}", e),
                }
            }
        }
        Err(e) => println!("✗ Failed to read directory: {}", e),
    }

    // Clean up
    for file in files.iter().rev() {
        let _ = fs::remove_file(file);
    }
    let _ = fs::remove_dir("/testdir/sub1/sub2");
    let _ = fs::remove_dir("/testdir/sub1");
    match fs::remove_dir(dirname) {
        Ok(_) => println!("✓ Cleaned up directory {}", dirname),
        Err(e) => println!("✗ Failed to remove directory: {}", e),
    }

    println!();
}

fn test_metadata_operations() {
    println!("Test 3: Metadata Operations");
    println!("---------------------------");

    let filename = "/metadata_test.txt";
    let content = "Test content for metadata";

    // Create file
    match fs::write(filename, content) {
        Ok(_) => println!("✓ Created {}", filename),
        Err(e) => {
            println!("✗ Failed to create file: {}", e);
            return;
        }
    }

    // Get metadata
    match fs::metadata(filename) {
        Ok(metadata) => {
            println!("✓ File metadata:");
            println!("  Size: {} bytes", metadata.len());
            println!("  Is file: {}", metadata.is_file());
            println!("  Is directory: {}", metadata.is_dir());
            println!("  Read-only: {}", metadata.permissions().readonly());
        }
        Err(e) => println!("✗ Failed to get metadata: {}", e),
    }

    // Truncate file
    match fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(filename)
    {
        Ok(_) => {
            println!("✓ Truncated file");
            match fs::metadata(filename) {
                Ok(metadata) => println!("  New size: {} bytes", metadata.len()),
                Err(e) => println!("  ✗ Failed to get metadata after truncate: {}", e),
            }
        }
        Err(e) => println!("✗ Failed to truncate file: {}", e),
    }

    // Clean up
    let _ = fs::remove_file(filename);
    println!();
}

fn test_rename_operations() {
    println!("Test 5: Rename/Move Operations");
    println!("------------------------------");

    // Rename a file
    let _ = fs::write("/rename_test.txt", "Hello, Monaka!");
    match fs::rename("/rename_test.txt", "/renamed.txt") {
        Ok(_) => println!("  Renamed /rename_test.txt -> /renamed.txt"),
        Err(e) => println!("  Failed to rename: {}", e),
    }
    match fs::metadata("/rename_test.txt") {
        Ok(_) => println!("  Old path still exists"),
        Err(_) => println!("  Old path no longer exists"),
    }
    match fs::read_to_string("/renamed.txt") {
        Ok(content) => println!("  /renamed.txt contains '{}'", content),
        Err(e) => println!("  Failed to read /renamed.txt: {}", e),
    }
    let _ = fs::remove_file("/renamed.txt");

    // Move file across directories
    fs::create_dir("/src_dir").unwrap();
    fs::create_dir("/dst_dir").unwrap();
    let _ = fs::write("/src_dir/data.txt", "moving data");
    match fs::rename("/src_dir/data.txt", "/dst_dir/data.txt") {
        Ok(_) => println!("  Moved /src_dir/data.txt -> /dst_dir/data.txt"),
        Err(e) => println!("  Failed to move: {}", e),
    }
    match fs::read_to_string("/dst_dir/data.txt") {
        Ok(content) => println!("  /dst_dir/data.txt contains '{}'", content),
        Err(e) => println!("  Failed to read: {}", e),
    }
    let _ = fs::remove_file("/dst_dir/data.txt");
    let _ = fs::remove_dir("/src_dir");
    let _ = fs::remove_dir("/dst_dir");

    // Rename a directory
    fs::create_dir("/mydir").unwrap();
    let _ = fs::write("/mydir/child.txt", "child");
    match fs::rename("/mydir", "/mydir_new") {
        Ok(_) => println!("  Renamed /mydir -> /mydir_new"),
        Err(e) => println!("  Failed to rename dir: {}", e),
    }
    match fs::read_to_string("/mydir_new/child.txt") {
        Ok(content) => println!("  /mydir_new/child.txt contains '{}'", content),
        Err(e) => println!("  Failed to read: {}", e),
    }
    let _ = fs::remove_file("/mydir_new/child.txt");
    let _ = fs::remove_dir("/mydir_new");

    // Overwrite rename
    let _ = fs::write("/src.txt", "new");
    let _ = fs::write("/dst.txt", "old");
    match fs::rename("/src.txt", "/dst.txt") {
        Ok(_) => println!("  Overwrite rename: /src.txt -> /dst.txt"),
        Err(e) => println!("  Failed to overwrite rename: {}", e),
    }
    match fs::read_to_string("/dst.txt") {
        Ok(content) => println!("  /dst.txt now contains '{}'", content),
        Err(e) => println!("  Failed to read: {}", e),
    }
    let _ = fs::remove_file("/dst.txt");

    // Error: rename non-existent file
    match fs::rename("/no_such_file.txt", "/new.txt") {
        Ok(_) => println!("  Should have failed for non-existent file"),
        Err(e) => println!("  Correctly handled missing file: {}", e),
    }

    println!();
}

fn test_error_handling() {
    println!("Test 4: Error Handling");
    println!("---------------------");

    // Try to read non-existent file
    match fs::read_to_string("/nonexistent.txt") {
        Ok(_) => println!("✗ Should have failed reading non-existent file"),
        Err(e) => println!("✓ Correctly handled missing file: {}", e),
    }

    // Try to remove non-existent file
    match fs::remove_file("/nonexistent.txt") {
        Ok(_) => println!("✗ Should have failed removing non-existent file"),
        Err(e) => println!("✓ Correctly handled removing missing file: {}", e),
    }

    // Try to create directory that already exists
    let dirname = "/test_dup";
    match fs::create_dir(dirname) {
        Ok(_) => {
            println!("✓ Created {}", dirname);
            match fs::create_dir(dirname) {
                Ok(_) => println!("✗ Should have failed creating duplicate directory"),
                Err(e) => println!("✓ Correctly handled duplicate directory: {}", e),
            }
            let _ = fs::remove_dir(dirname);
        }
        Err(e) => println!("✗ Failed to create test directory: {}", e),
    }

    // Try to read directory as file
    match fs::create_dir("/dirtest") {
        Ok(_) => {
            match fs::read_to_string("/dirtest") {
                Ok(_) => println!("✗ Should have failed reading directory as file"),
                Err(e) => println!("✓ Correctly handled reading directory as file: {}", e),
            }
            let _ = fs::remove_dir("/dirtest");
        }
        Err(e) => println!("✗ Failed to create test directory: {}", e),
    }

    println!();
}
