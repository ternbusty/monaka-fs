use chrono::{DateTime, Utc};
use fs_wasm::{
    fs_close, fs_fstat, fs_mkdir, fs_open_path, fs_open_path_with_flags, fs_read, fs_seek, fs_write,
    FsStat, O_APPEND, O_CREAT, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY,
};

fn main() {
    println!("Rust Example - Using fs-wasm as Library");
    println!("========================================\n");

    // Run demo operations
    if let Err(e) = run_demos() {
        eprintln!("Demo failed: {}", e);
        return;
    }

    println!("\nAll operations completed successfully!");
}

fn run_demos() -> Result<(), String> {
    demo_basic_file_operations()?;
    demo_directory_operations()?;
    demo_metadata_operations()?;
    demo_seek_operations()?;
    demo_append_operations()?;
    demo_trunc_operations()?;
    Ok(())
}

// Demo 1: Basic file operations (write and read)
fn demo_basic_file_operations() -> Result<(), String> {
    println!("=== Demo 1: Basic File Operations ===");

    // Create a file
    let path = "/hello.txt";
    let fd = fs_open_path(path.as_ptr(), path.len() as u32);
    if fd <= 0 {
        return Err(format!("Failed to open file: {}", fd));
    }
    println!("Opened file: {} (fd={})", path, fd);

    // Write data
    let content = "Hello from rust-example!";
    let written = fs_write(fd as u32, content.as_ptr(), content.len() as u32);
    if written != content.len() as i32 {
        fs_close(fd as u32);
        return Err(format!("Write failed: expected {}, got {}", content.len(), written));
    }
    println!("Wrote {} bytes: \"{}\"", written, content);

    // Get file metadata using fstat
    let mut stat = FsStat {
        size: 0,
        created: 0,
        modified: 0,
    };
    let result = fs_fstat(fd as u32, &mut stat as *mut FsStat);
    if result != 0 {
        fs_close(fd as u32);
        return Err(format!("fstat failed: {}", result));
    }
    println!("File size: {} bytes", stat.size);

    // Convert timestamps to ISO 8601 format
    let created = DateTime::<Utc>::from_timestamp(stat.created as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "Invalid timestamp".to_string());
    let modified = DateTime::<Utc>::from_timestamp(stat.modified as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "Invalid timestamp".to_string());

    println!("Created at: {}", created);
    println!("Modified at: {}", modified);

    // Seek to beginning
    let seek_result = fs_seek(fd as u32, 0, 0); // SEEK_SET = 0
    if seek_result < 0 {
        fs_close(fd as u32);
        return Err(format!("Seek failed: {}", seek_result));
    }

    // Read data back with exact buffer size
    let mut buffer = vec![0u8; stat.size as usize];
    let read_bytes = fs_read(fd as u32, buffer.as_mut_ptr(), buffer.len() as u32);
    if read_bytes < 0 {
        fs_close(fd as u32);
        return Err(format!("Read failed: {}", read_bytes));
    }

    let read_content = String::from_utf8_lossy(&buffer);
    println!("Read {} bytes: \"{}\"", read_bytes, read_content);

    // Verify content matches
    if read_content != content {
        fs_close(fd as u32);
        return Err(format!("Content mismatch: expected '{}', got '{}'", content, read_content));
    }
    println!("Content verification: OK");

    // Close file
    fs_close(fd as u32);
    println!("File closed\n");

    Ok(())
}

// Demo 2: Directory operations
fn demo_directory_operations() -> Result<(), String> {
    println!("=== Demo 2: Directory Operations ===");

    // Create nested directories
    let dirs = ["/data", "/data/logs", "/data/config"];
    for dir in &dirs {
        let result = fs_mkdir(dir.as_ptr(), dir.len() as u32);
        if result != 0 {
            return Err(format!("Failed to create directory {}: {}", dir, result));
        }
        println!("Created directory: {}", dir);
    }

    // Create files in directories
    let files = [
        ("/data/app.log", "Application started"),
        ("/data/logs/debug.log", "Debug information"),
        ("/data/config/settings.txt", "key=value"),
    ];

    for (path, content) in &files {
        let fd = fs_open_path(path.as_ptr(), path.len() as u32);
        if fd <= 0 {
            return Err(format!("Failed to open file {}: {}", path, fd));
        }

        let written = fs_write(fd as u32, content.as_ptr(), content.len() as u32);
        fs_close(fd as u32);

        if written != content.len() as i32 {
            return Err(format!("Failed to write to {}", path));
        }
        println!("Created file: {} ({} bytes)", path, written);
    }

    println!();
    Ok(())
}

// Demo 3: Metadata operations
fn demo_metadata_operations() -> Result<(), String> {
    println!("=== Demo 3: Metadata Operations ===");

    // Create a file with known size
    let path = "/metadata_test.dat";
    let fd = fs_open_path(path.as_ptr(), path.len() as u32);
    if fd <= 0 {
        return Err(format!("Failed to open file: {}", fd));
    }

    // Write data
    let data = b"0123456789ABCDEF"; // 16 bytes
    let written = fs_write(fd as u32, data.as_ptr(), data.len() as u32);
    if written != data.len() as i32 {
        fs_close(fd as u32);
        return Err(format!("Write failed: {}", written));
    }
    println!("Wrote {} bytes to {}", written, path);

    // Get file metadata
    let mut stat = FsStat {
        size: 0,
        created: 0,
        modified: 0,
    };
    let result = fs_fstat(fd as u32, &mut stat as *mut FsStat);
    if result != 0 {
        fs_close(fd as u32);
        return Err(format!("fstat failed: {}", result));
    }
    println!("File size from fstat: {} bytes", stat.size);

    // Convert timestamps to ISO 8601 format
    let created = DateTime::<Utc>::from_timestamp(stat.created as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "Invalid timestamp".to_string());
    let modified = DateTime::<Utc>::from_timestamp(stat.modified as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "Invalid timestamp".to_string());

    println!("Created at: {}", created);
    println!("Modified at: {}", modified);

    // Verify size matches
    if stat.size != data.len() as u64 {
        fs_close(fd as u32);
        return Err(format!(
            "Size mismatch: expected {}, got {}",
            data.len(),
            stat.size
        ));
    }
    println!("Size verification: OK");

    fs_close(fd as u32);
    println!();
    Ok(())
}

// Demo 4: Seek operations
fn demo_seek_operations() -> Result<(), String> {
    println!("=== Demo 4: Seek Operations ===");

    let path = "/seek_test.txt";
    let fd = fs_open_path(path.as_ptr(), path.len() as u32);
    if fd <= 0 {
        return Err(format!("Failed to open file: {}", fd));
    }

    // Write a known pattern
    let full_content = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    fs_write(fd as u32, full_content.as_ptr(), full_content.len() as u32);
    println!("Wrote: {}", full_content);

    // Test SEEK_SET (from beginning)
    fs_seek(fd as u32, 5, 0); // Seek to position 5
    let mut buffer = vec![0u8; 5];
    fs_read(fd as u32, buffer.as_mut_ptr(), 5);
    println!("SEEK_SET to 5: Read '{}'", String::from_utf8_lossy(&buffer));

    // Test SEEK_SET to different position
    fs_seek(fd as u32, 10, 0); // Seek to position 10
    fs_read(fd as u32, buffer.as_mut_ptr(), 5);
    println!("SEEK_SET to 10: Read '{}'", String::from_utf8_lossy(&buffer));

    // Test reading from beginning again
    fs_seek(fd as u32, 0, 0);
    fs_read(fd as u32, buffer.as_mut_ptr(), 5);
    println!("SEEK_SET to 0: Read '{}'", String::from_utf8_lossy(&buffer));

    fs_close(fd as u32);
    println!();
    Ok(())
}

// Demo 5: O_APPEND operations
fn demo_append_operations() -> Result<(), String> {
    println!("=== Demo 5: O_APPEND Operations ===");

    let path = "/append_test.txt";

    // Create file with initial content
    let fd = fs_open_path_with_flags(path.as_ptr(), path.len() as u32, O_RDWR | O_CREAT);
    if fd <= 0 {
        return Err(format!("Failed to open file: {}", fd));
    }
    fs_write(fd as u32, b"Initial".as_ptr(), 7);
    fs_close(fd as u32);
    println!("Created file with 'Initial' content");

    // Open in append mode
    let fd = fs_open_path_with_flags(path.as_ptr(), path.len() as u32, O_WRONLY | O_APPEND);
    if fd <= 0 {
        return Err(format!("Failed to open file in append mode: {}", fd));
    }

    // Write multiple times - all should append
    fs_write(fd as u32, b" First".as_ptr(), 6);
    fs_write(fd as u32, b" Second".as_ptr(), 7);
    fs_write(fd as u32, b" Third".as_ptr(), 6);
    println!("Appended three strings");

    fs_close(fd as u32);

    // Verify content
    let fd = fs_open_path_with_flags(path.as_ptr(), path.len() as u32, O_RDONLY);
    if fd <= 0 {
        return Err(format!("Failed to open file for reading: {}", fd));
    }

    let mut buffer = vec![0u8; 100];
    let read_bytes = fs_read(fd as u32, buffer.as_mut_ptr(), buffer.len() as u32);
    if read_bytes < 0 {
        fs_close(fd as u32);
        return Err(format!("Read failed: {}", read_bytes));
    }

    let content = String::from_utf8_lossy(&buffer[..read_bytes as usize]);
    println!("Final content: '{}'", content);

    if content != "Initial First Second Third" {
        fs_close(fd as u32);
        return Err(format!("Content mismatch: expected 'Initial First Second Third', got '{}'", content));
    }

    fs_close(fd as u32);
    println!("Append verification: OK\n");
    Ok(())
}

// Demo 6: O_TRUNC operations
fn demo_trunc_operations() -> Result<(), String> {
    println!("=== Demo 6: O_TRUNC Operations ===");

    let path = "/trunc_test.txt";

    // Create file with old content
    let fd = fs_open_path_with_flags(path.as_ptr(), path.len() as u32, O_RDWR | O_CREAT);
    if fd <= 0 {
        return Err(format!("Failed to create file: {}", fd));
    }
    let old_content = "This is very long old content that should be truncated";
    fs_write(fd as u32, old_content.as_ptr(), old_content.len() as u32);
    fs_close(fd as u32);
    println!("Created file with old content ({} bytes)", old_content.len());

    // Open with O_TRUNC
    let fd = fs_open_path_with_flags(path.as_ptr(), path.len() as u32, O_RDWR | O_TRUNC);
    if fd <= 0 {
        return Err(format!("Failed to open file with O_TRUNC: {}", fd));
    }
    println!("Opened file with O_TRUNC flag");

    // File should be empty now
    let mut buffer = vec![0u8; 100];
    let read_bytes = fs_read(fd as u32, buffer.as_mut_ptr(), buffer.len() as u32);
    if read_bytes != 0 {
        fs_close(fd as u32);
        return Err(format!("Expected empty file, but read {} bytes", read_bytes));
    }
    println!("Verified file is empty after O_TRUNC");

    // Write new content
    let new_content = "New content";
    fs_write(fd as u32, new_content.as_ptr(), new_content.len() as u32);
    println!("Wrote new content: '{}'", new_content);

    // Read back
    fs_seek(fd as u32, 0, 0);
    let read_bytes = fs_read(fd as u32, buffer.as_mut_ptr(), buffer.len() as u32);
    if read_bytes < 0 {
        fs_close(fd as u32);
        return Err(format!("Read failed: {}", read_bytes));
    }

    let content = String::from_utf8_lossy(&buffer[..read_bytes as usize]);
    if content != new_content {
        fs_close(fd as u32);
        return Err(format!("Content mismatch: expected '{}', got '{}'", new_content, content));
    }

    fs_close(fd as u32);
    println!("Truncate and write verification: OK\n");
    Ok(())
}
