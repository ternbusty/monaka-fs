//! HTTP Cache Request Handler
//!
//! WASM component that handles API requests with caching.
//! - Checks cache in /cache/{path}.cache
//! - If cache miss or expired, fetches from external API (mock for now)
//! - Saves response to cache with TTL

use serde::{Deserialize, Serialize};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_DIR: &str = "/cache";
const DEFAULT_TTL: u64 = 300; // 5 minutes

#[derive(Serialize, Deserialize, Debug)]
struct CacheEntry {
    cached_at: u64,
    ttl_seconds: u64,
    data: serde_json::Value,
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn get_cache_path(api_path: &str) -> String {
    let safe_name = api_path
        .trim_start_matches('/')
        .replace('/', "_")
        .replace('?', "_")
        .replace('&', "_");
    format!("{}/{}.cache", CACHE_DIR, safe_name)
}

fn check_cache(api_path: &str) -> Option<serde_json::Value> {
    let cache_path = get_cache_path(api_path);

    match fs::read_to_string(&cache_path) {
        Ok(content) => match serde_json::from_str::<CacheEntry>(&content) {
            Ok(entry) => {
                let now = current_timestamp();
                if now < entry.cached_at + entry.ttl_seconds {
                    let remaining = entry.cached_at + entry.ttl_seconds - now;
                    println!("[CACHE HIT] {} (expires in {}s)", api_path, remaining);
                    Some(entry.data)
                } else {
                    println!("[CACHE EXPIRED] {}", api_path);
                    None
                }
            }
            Err(e) => {
                println!("[CACHE PARSE ERROR] {}: {}", cache_path, e);
                None
            }
        },
        Err(_) => {
            println!("[CACHE MISS] {}", api_path);
            None
        }
    }
}

fn write_cache(api_path: &str, data: &serde_json::Value) {
    // Ensure cache directory exists
    let _ = fs::create_dir(CACHE_DIR);

    let cache_path = get_cache_path(api_path);
    let entry = CacheEntry {
        cached_at: current_timestamp(),
        ttl_seconds: DEFAULT_TTL,
        data: data.clone(),
    };

    match serde_json::to_string_pretty(&entry) {
        Ok(json) => match fs::write(&cache_path, &json) {
            Ok(_) => println!("[CACHE WRITE] {}", cache_path),
            Err(e) => eprintln!("[CACHE WRITE ERROR] {}: {}", cache_path, e),
        },
        Err(e) => eprintln!("[CACHE SERIALIZE ERROR] {}", e),
    }
}

fn fetch_from_api(api_path: &str) -> serde_json::Value {
    // Mock external API response
    // In a real implementation, this would use wasi-http
    println!("[API FETCH] {} (mock)", api_path);

    let timestamp = current_timestamp();
    serde_json::json!({
        "path": api_path,
        "timestamp": timestamp,
        "data": {
            "message": format!("Response for {}", api_path),
            "items": [1, 2, 3, 4, 5]
        }
    })
}

fn handle_request(api_path: &str) -> String {
    // 1. Check cache
    if let Some(cached_data) = check_cache(api_path) {
        return serde_json::to_string_pretty(&cached_data).unwrap_or_default();
    }

    // 2. Fetch from external API
    let data = fetch_from_api(api_path);

    // 3. Write to cache
    write_cache(api_path, &data);

    // 4. Return response
    serde_json::to_string_pretty(&data).unwrap_or_default()
}

fn main() {
    // Read API path from environment variable
    let api_path = std::env::var("API_PATH").unwrap_or_else(|_| "/api/default".to_string());

    println!("=== HTTP Cache Handler ===");
    println!("Request: {}", api_path);
    println!();

    let response = handle_request(&api_path);

    println!();
    println!("Response:");
    println!("{}", response);
}
