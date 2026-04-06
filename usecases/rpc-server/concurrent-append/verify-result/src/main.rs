//! Verify Concurrent Append Results
//!
//! Reads the shared file and verifies:
//! 1. Total line count matches expected (clients × appends per client)
//! 2. Each line has valid format (CLIENT_XXX:SEQ_XXXXX)
//! 3. No data corruption (partial lines, mixed content)
//!
//! Usage:
//!   EXPECTED_CLIENTS=4 APPEND_COUNT=100 wasmtime run ... verify-result.wasm
//!   SHOW_LINES=20 wasmtime run ... verify-result.wasm  # Just show first 20 lines

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

fn main() {
    // If SHOW_LINES is set, just print that many lines and exit
    if let Ok(show_lines) = std::env::var("SHOW_LINES") {
        if let Ok(n) = show_lines.parse::<usize>() {
            show_file_head(n);
            return;
        }
    }

    let expected_clients: u32 = std::env::var("EXPECTED_CLIENTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    let append_count: u32 = std::env::var("APPEND_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let shared_file = "/shared/concurrent.log";
    let expected_total = expected_clients * append_count;

    println!("=== Concurrent Append Verification ===");
    println!("Expected: {} clients × {} appends = {} lines", expected_clients, append_count, expected_total);
    println!();

    let file = match File::open(shared_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("ERROR: Failed to open {}: {}", shared_file, e);
            std::process::exit(1);
        }
    };

    let reader = BufReader::new(file);
    let mut total_lines = 0u32;
    let mut valid_lines = 0u32;
    let mut invalid_lines = 0u32;
    let mut client_counts: HashMap<u32, u32> = HashMap::new();
    let mut corrupted_samples: Vec<String> = Vec::new();

    for line_result in reader.lines() {
        total_lines += 1;

        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Read error at line {}: {}", total_lines, e);
                invalid_lines += 1;
                continue;
            }
        };

        // Expected format: [timestamp] CLIENT_XXX:SEQ_XXXXX
        // Extract the CLIENT_XXX:SEQ_XXXXX part (after timestamp)
        let content = if line.starts_with('[') {
            line.split(']').nth(1).map(|s| s.trim()).unwrap_or(&line)
        } else {
            &line
        };

        if let Some((client_part, seq_part)) = content.split_once(':') {
            if client_part.starts_with("CLIENT_") && seq_part.starts_with("SEQ_") {
                if let Ok(client_id) = client_part.trim_start_matches("CLIENT_").parse::<u32>() {
                    *client_counts.entry(client_id).or_insert(0) += 1;
                    valid_lines += 1;
                    continue;
                }
            }
        }

        // Invalid format
        invalid_lines += 1;
        if corrupted_samples.len() < 5 {
            corrupted_samples.push(format!("Line {}: {:?}", total_lines, line));
        }
    }

    // Print results
    println!("--- Results ---");
    println!("Total lines:   {}", total_lines);
    println!("Valid lines:   {}", valid_lines);
    println!("Invalid lines: {}", invalid_lines);
    println!();

    println!("--- Per-Client Counts ---");
    let mut client_ids: Vec<_> = client_counts.keys().collect();
    client_ids.sort();
    for client_id in client_ids {
        let count = client_counts[client_id];
        let status = if count == append_count { "OK" } else { "MISMATCH" };
        println!("  Client {:3}: {:5} lines [{}]", client_id, count, status);
    }
    println!();

    if !corrupted_samples.is_empty() {
        println!("--- Corrupted Line Samples ---");
        for sample in &corrupted_samples {
            println!("  {}", sample);
        }
        println!();
    }

    // Final verdict
    let all_clients_ok = client_counts.len() as u32 == expected_clients
        && client_counts.values().all(|&c| c == append_count);
    let no_corruption = invalid_lines == 0;
    let count_matches = total_lines == expected_total;

    println!("=== Verification Result ===");
    if all_clients_ok && no_corruption && count_matches {
        println!("PASS: All {} lines verified, no data corruption", total_lines);
        println!();
        println!("Concurrent append with proper locking: CONFIRMED");
    } else {
        println!("FAIL:");
        if !count_matches {
            println!("  - Line count mismatch: expected {}, got {}", expected_total, total_lines);
        }
        if !all_clients_ok {
            println!("  - Client count mismatch or uneven distribution");
        }
        if !no_corruption {
            println!("  - {} corrupted/invalid lines detected", invalid_lines);
        }
        std::process::exit(1);
    }
}

/// Show first N lines of the file
fn show_file_head(n: usize) {
    let shared_file = "/shared/concurrent.log";

    let file = match File::open(shared_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open {}: {}", shared_file, e);
            return;
        }
    };

    let reader = BufReader::new(file);
    for (i, line) in reader.lines().enumerate() {
        if i >= n {
            break;
        }
        if let Ok(line) = line {
            println!("{}", line);
        }
    }
}
