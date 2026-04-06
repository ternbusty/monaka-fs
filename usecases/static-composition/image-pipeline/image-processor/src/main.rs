//! Image Processing Pipeline
//!
//! Demonstrates VFS usage with intermediate files:
//! 1. Read input "image"
//! 2. Resize (simulate) and write intermediate file
//! 3. Read intermediate, convert format, write output
//!
//! This showcases how wac-composed WASM components can use
//! the VFS adapter for file-based data pipelines.

use std::fs;

fn main() {
    println!("=== Image Processing Pipeline ===");
    println!();

    // Setup directories
    fs::create_dir_all("/input").ok();
    fs::create_dir_all("/work").ok();
    fs::create_dir_all("/output").ok();

    // Create sample input (simulated raw image)
    let input_data = create_sample_image();
    fs::write("/input/photo.raw", &input_data).expect("Failed to write input");
    println!(
        "Created input: /input/photo.raw ({} bytes)",
        input_data.len()
    );
    println!();

    // Step 1: Resize
    println!("--- Step 1: Resize ---");
    resize_image("/input/photo.raw", "/work/resized.dat");

    // Step 2: Convert format
    println!();
    println!("--- Step 2: Convert Format ---");
    convert_to_png("/work/resized.dat", "/output/photo.png");

    // Show result
    println!();
    println!("=== Pipeline Complete ===");
    let output = fs::read("/output/photo.png").expect("Failed to read output");
    println!("Output: /output/photo.png ({} bytes)", output.len());

    // Verify PNG magic bytes
    if output.len() >= 8 && &output[0..4] == &[0x89, b'P', b'N', b'G'] {
        println!("PNG header verified!");
    }
}

/// Create a simulated raw image with header and pixel data
fn create_sample_image() -> Vec<u8> {
    let mut data = b"RAWIMG".to_vec(); // 6-byte header
                                       // Generate 1KB of "pixel" data (repeating pattern)
    data.extend((0u8..=255).cycle().take(1024));
    data
}

/// Simulate image resize by downsampling
fn resize_image(input_path: &str, output_path: &str) {
    let input = fs::read(input_path).expect("Failed to read input");
    println!("[RESIZE] Input: {} bytes", input.len());

    // Skip header, take every 4th byte to simulate 4x downscale
    let header_len = 6; // "RAWIMG"
    let pixels = &input[header_len..];
    let resized: Vec<u8> = pixels
        .chunks(4)
        .filter_map(|chunk| chunk.first().copied())
        .collect();

    println!(
        "[RESIZE] Downscaled {} -> {} pixels",
        pixels.len(),
        resized.len()
    );

    // Write with intermediate format header
    let mut output = b"RESIZED:".to_vec(); // 8-byte header
    output.extend(&(resized.len() as u32).to_le_bytes()); // 4-byte size
    output.extend(resized);

    fs::write(output_path, &output).expect("Failed to write");
    println!("[RESIZE] Output: {} ({} bytes)", output_path, output.len());
}

/// Simulate format conversion to PNG
fn convert_to_png(input_path: &str, output_path: &str) {
    let input = fs::read(input_path).expect("Failed to read input");
    println!("[CONVERT] Input: {} bytes", input.len());

    // Parse intermediate format: "RESIZED:" (8) + size (4) + data
    let header_len = 8 + 4;
    if input.len() < header_len {
        panic!("Invalid intermediate file format");
    }

    let pixel_data = &input[header_len..];
    println!("[CONVERT] Pixel data: {} bytes", pixel_data.len());

    // Create simulated PNG with proper magic bytes
    let mut output = vec![
        0x89, b'P', b'N', b'G', // PNG signature (first 4 bytes)
        0x0D, 0x0A, 0x1A, 0x0A, // PNG signature (next 4 bytes)
    ];

    // Add fake IHDR chunk marker
    output.extend(b"IHDR");

    // Add pixel data
    output.extend(pixel_data);

    // Add fake IEND chunk marker
    output.extend(b"IEND");

    fs::write(output_path, &output).expect("Failed to write");
    println!("[CONVERT] Output: {} ({} bytes)", output_path, output.len());
}
