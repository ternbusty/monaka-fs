//! CI Cache Job
//!
//! WASM component that simulates a CI job fetching dependency caches.
//! Multiple jobs can run concurrently, sharing cache via VFS RPC server.
//! Uses directory-based locking for exclusive access per library.

use std::fs;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CACHE_DIR: &str = "/cache";
const LOCK_RETRY_MS: u64 = 100;
const MAX_RETRIES: u32 = 50;

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn main() {
    let job_id = std::env::var("JOB_ID").unwrap_or_else(|_| "?".to_string());
    let deps_str = std::env::var("DEPS").unwrap_or_default();
    let deps: Vec<&str> = deps_str.split(',').filter(|s| !s.is_empty()).collect();

    if deps.is_empty() {
        println!("[Job{}] No dependencies specified", job_id);
        return;
    }

    println!("[Job{}] Starting with deps: {}", job_id, deps.join(", "));

    // Ensure cache directory exists
    if let Err(e) = fs::create_dir_all(CACHE_DIR) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            eprintln!("[Job{}] Failed to create cache dir: {}", job_id, e);
        }
    }

    for dep in deps {
        process_dependency(&job_id, dep);
    }

    println!("[Job{}] Done", job_id);
}

fn process_dependency(job_id: &str, dep: &str) {
    let cache_file = format!("{}/{}.cache", CACHE_DIR, dep);
    let lock_dir = format!("{}/{}.lock", CACHE_DIR, dep);

    // 1. Acquire lock
    if !acquire_lock(&lock_dir, job_id, dep) {
        eprintln!("[Job{}] {}: Failed to acquire lock, skipping", job_id, dep);
        return;
    }

    // 2. Check cache
    match fs::read_to_string(&cache_file) {
        Ok(content) => {
            println!("[Job{}] {}: HIT ({} bytes)", job_id, dep, content.len());
        }
        Err(_) => {
            println!("[Job{}] {}: MISS - downloading...", job_id, dep);

            // Simulate download time
            thread::sleep(Duration::from_millis(500));

            // Write cache (simulated library content)
            let content = format!(
                "library:{}\nversion:{}\ncached_at:{}\ncached_by:Job{}",
                dep.split('-').next().unwrap_or(dep),
                dep.split('-').last().unwrap_or("unknown"),
                current_timestamp(),
                job_id
            );

            match fs::write(&cache_file, &content) {
                Ok(_) => println!("[Job{}] {}: cached ({} bytes)", job_id, dep, content.len()),
                Err(e) => eprintln!("[Job{}] {}: write failed: {}", job_id, dep, e),
            }
        }
    }

    // 3. Release lock
    release_lock(&lock_dir, job_id, dep);
}

fn acquire_lock(lock_dir: &str, job_id: &str, dep: &str) -> bool {
    println!("[Job{}] {}: acquiring lock...", job_id, dep);

    for retry in 0..MAX_RETRIES {
        match fs::create_dir(lock_dir) {
            Ok(_) => {
                println!("[Job{}] {}: lock acquired", job_id, dep);
                return true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if retry > 0 && retry % 5 == 0 {
                    println!("[Job{}] {}: waiting (retry {})...", job_id, dep, retry);
                }
                thread::sleep(Duration::from_millis(LOCK_RETRY_MS));
            }
            Err(e) => {
                eprintln!("[Job{}] {}: lock error: {}", job_id, dep, e);
                return false;
            }
        }
    }

    eprintln!(
        "[Job{}] {}: timeout acquiring lock after {} retries",
        job_id, dep, MAX_RETRIES
    );
    false
}

fn release_lock(lock_dir: &str, job_id: &str, dep: &str) {
    match fs::remove_dir(lock_dir) {
        Ok(_) => println!("[Job{}] {}: lock released", job_id, dep),
        Err(e) => eprintln!("[Job{}] {}: failed to release lock: {}", job_id, dep, e),
    }
}
