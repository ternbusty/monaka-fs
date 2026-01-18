//! Host Trait Concurrent Append Test
//!
//! Runs multiple WASM instances in parallel threads, all sharing the same VFS.
//! This tests fs-core's locking implementation (DashMap + per-inode RwLock).
//!
//! Usage:
//!   host-concurrent-runner [num_clients] [append_count]
//!   host-concurrent-runner 3 50

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use vfs_host::{add_to_linker_with_vfs, VfsHostState};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::bindings::sync::Command;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let num_clients: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let append_count: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);

    println!("==============================================");
    println!("  Host Trait Concurrent Append Test");
    println!("==============================================");
    println!();
    println!("Configuration:");
    println!("  Clients:         {}", num_clients);
    println!("  Appends/client:  {}", append_count);
    println!("  Expected lines:  {}", num_clients * append_count);
    println!();

    // Initialize wasmtime
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Arc::new(Engine::new(&config)?);

    // Load the append-client WASM component
    let wasm_path = std::env::var("WASM_PATH")
        .unwrap_or_else(|_| "../append-client/target/wasm32-wasip2/release/append-client.wasm".to_string());
    let component = Arc::new(
        Component::from_file(&engine, &wasm_path)
            .context(format!("Failed to load WASM from {}", wasm_path))?,
    );

    // Create shared VFS
    let vfs_host_state = VfsHostState::new().context("Failed to create VfsHostState")?;
    let shared_vfs = vfs_host_state.get_shared_vfs();

    // Create /shared directory
    shared_vfs
        .mkdir_p("/shared")
        .map_err(|e| anyhow::anyhow!("Failed to create /shared: {:?}", e))?;

    println!("Starting {} threads with shared VFS...", num_clients);
    println!();

    // Track errors
    let error_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Spawn worker threads
    for client_id in 1..=num_clients {
        let engine = Arc::clone(&engine);
        let component = Arc::clone(&component);
        let vfs = Arc::clone(&shared_vfs);
        let errors = Arc::clone(&error_count);
        let append_count_str = append_count.to_string();
        let client_id_str = client_id.to_string();

        handles.push(thread::spawn(move || {
            run_wasm_instance(
                &engine,
                &component,
                vfs,
                &client_id_str,
                &append_count_str,
                errors,
            )
        }));
    }

    // Wait for all threads
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let total_errors = error_count.load(Ordering::Relaxed);
    println!();

    // Verify results
    println!("--- Verification ---");
    let result = verify_results(&shared_vfs, num_clients, append_count)?;

    // Show first 20 lines
    println!();
    println!("--- First 20 lines ---");
    show_file_head(&shared_vfs, 20)?;

    println!();
    if result.is_pass && total_errors == 0 {
        println!("==============================================");
        println!("  TEST PASSED");
        println!("==============================================");
        println!();
        println!("True concurrent access with proper locking: CONFIRMED");
    } else {
        println!("==============================================");
        println!("  TEST FAILED");
        println!("==============================================");
        if total_errors > 0 {
            println!("  WASM execution errors: {}", total_errors);
        }
        std::process::exit(1);
    }

    Ok(())
}

fn run_wasm_instance(
    engine: &Engine,
    component: &Component,
    shared_vfs: Arc<vfs_host::Fs>,
    client_id: &str,
    append_count: &str,
    error_count: Arc<AtomicUsize>,
) {
    // Set up environment variables
    let env_vars = [
        ("CLIENT_ID", client_id),
        ("APPEND_COUNT", append_count),
    ];

    // Create VfsHostState with shared VFS
    let vfs_host_state = VfsHostState::from_shared_vfs_with_env(shared_vfs, &env_vars);

    // Create store and linker
    let mut store = Store::new(engine, vfs_host_state);
    let mut linker = wasmtime::component::Linker::new(engine);

    if let Err(e) = add_to_linker_with_vfs(&mut linker) {
        eprintln!("[Thread {}] Failed to add VFS to linker: {}", client_id, e);
        error_count.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Instantiate WASM
    let command = match Command::instantiate(&mut store, component, &linker) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("[Thread {}] Failed to instantiate WASM: {}", client_id, e);
            error_count.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Run WASM
    match command.wasi_cli_run().call_run(&mut store) {
        Ok(Ok(())) => {
            // Success is printed by the WASM itself
        }
        Ok(Err(())) => {
            eprintln!("[Thread {}] WASM returned error", client_id);
            error_count.fetch_add(1, Ordering::Relaxed);
        }
        Err(e) => {
            eprintln!("[Thread {}] WASM execution failed: {:?}", client_id, e);
            error_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

struct VerifyResult {
    total_lines: usize,
    valid_lines: usize,
    invalid_lines: usize,
    is_pass: bool,
}

fn verify_results(
    vfs: &vfs_host::Fs,
    expected_clients: usize,
    expected_appends: usize,
) -> Result<VerifyResult> {
    let path = "/shared/concurrent.log";

    // Open and read file
    let fd = vfs
        .open_path(path)
        .map_err(|e| anyhow::anyhow!("Failed to open {}: {:?}", path, e))?;

    // Read all content
    let mut content = vec![0u8; 1024 * 1024]; // 1MB buffer
    let bytes_read = vfs
        .read(fd, &mut content)
        .map_err(|e| anyhow::anyhow!("Failed to read: {:?}", e))?;
    vfs.close(fd)
        .map_err(|e| anyhow::anyhow!("Failed to close: {:?}", e))?;

    let content = String::from_utf8_lossy(&content[..bytes_read]);

    // Parse and verify
    let mut total_lines = 0usize;
    let mut valid_lines = 0usize;
    let mut invalid_lines = 0usize;
    let mut client_counts: HashMap<u32, usize> = HashMap::new();

    for line in content.lines() {
        total_lines += 1;

        // Format: [timestamp] CLIENT_XXX:SEQ_XXXXX
        let content_part = if line.starts_with('[') {
            line.split(']').nth(1).map(|s| s.trim()).unwrap_or(line)
        } else {
            line
        };

        if let Some((client_part, seq_part)) = content_part.split_once(':') {
            if client_part.starts_with("CLIENT_") && seq_part.starts_with("SEQ_") {
                if let Ok(client_id) = client_part.trim_start_matches("CLIENT_").parse::<u32>() {
                    *client_counts.entry(client_id).or_insert(0) += 1;
                    valid_lines += 1;
                    continue;
                }
            }
        }

        invalid_lines += 1;
    }

    let expected_total = expected_clients * expected_appends;
    println!("Total lines:   {}", total_lines);
    println!("Valid lines:   {}", valid_lines);
    println!("Invalid lines: {}", invalid_lines);
    println!();

    println!("--- Per-Client Counts ---");
    let mut client_ids: Vec<_> = client_counts.keys().collect();
    client_ids.sort();
    for client_id in &client_ids {
        let count = client_counts[client_id];
        let status = if count == expected_appends { "OK" } else { "MISMATCH" };
        println!("  Client {:3}: {:5} lines [{}]", client_id, count, status);
    }

    let all_clients_ok = client_counts.len() == expected_clients
        && client_counts.values().all(|&c| c == expected_appends);
    let is_pass = total_lines == expected_total && invalid_lines == 0 && all_clients_ok;

    Ok(VerifyResult {
        total_lines,
        valid_lines,
        invalid_lines,
        is_pass,
    })
}

fn show_file_head(vfs: &vfs_host::Fs, n: usize) -> Result<()> {
    let path = "/shared/concurrent.log";

    let fd = vfs
        .open_path(path)
        .map_err(|e| anyhow::anyhow!("Failed to open {}: {:?}", path, e))?;

    let mut content = vec![0u8; 1024 * 1024];
    let bytes_read = vfs
        .read(fd, &mut content)
        .map_err(|e| anyhow::anyhow!("Failed to read: {:?}", e))?;
    vfs.close(fd)
        .map_err(|e| anyhow::anyhow!("Failed to close: {:?}", e))?;

    let content = String::from_utf8_lossy(&content[..bytes_read]);
    for (i, line) in content.lines().enumerate() {
        if i >= n {
            break;
        }
        println!("{}", line);
    }

    Ok(())
}
