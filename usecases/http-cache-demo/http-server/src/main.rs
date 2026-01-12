//! HTTP Cache Server
//!
//! Real HTTP server using axum that invokes WASM handlers for each request.
//! WASM handlers share a VFS for caching across requests.

use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use std::sync::{Arc, Mutex};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, Store};

/// Shared application state
struct AppState {
    engine: Engine,
    handler_component: Component,
    shared_vfs: Arc<Mutex<vfs_host::SharedVfsCore>>,
}

/// Handle API requests
async fn handle_api_request(
    State(state): State<Arc<AppState>>,
    Path(api_path): Path<String>,
) -> impl IntoResponse {
    let api_path = format!("/api/{}", api_path);
    let engine = state.engine.clone();
    let handler_component = state.handler_component.clone();
    let shared_vfs = Arc::clone(&state.shared_vfs);

    // Run WASM handler in blocking thread pool
    let result = tokio::task::spawn_blocking(move || {
        run_wasm_handler(&engine, &handler_component, shared_vfs, &api_path)
    })
    .await;

    match result {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => format!("{{\"error\": \"{}\"}}", e),
        Err(e) => format!("{{\"error\": \"Task failed: {}\"}}", e),
    }
}

/// Run WASM handler (called from spawn_blocking)
fn run_wasm_handler(
    engine: &Engine,
    handler_component: &Component,
    shared_vfs: Arc<Mutex<vfs_host::SharedVfsCore>>,
    api_path: &str,
) -> Result<String> {
    println!("[SERVER] Handling request: {}", api_path);

    // Create VfsHostState from shared VFS with environment variable
    let vfs_state =
        vfs_host::VfsHostState::from_shared_vfs_with_env(shared_vfs, &[("API_PATH", api_path)]);

    // Create store with VFS state
    let mut store = Store::new(engine, vfs_state);

    // Create linker and add VFS bindings
    let mut linker = wasmtime::component::Linker::new(engine);
    vfs_host::add_to_linker_with_vfs(&mut linker)?;

    // Instantiate handler
    use wasmtime_wasi::bindings::sync::Command;
    let command = Command::instantiate(&mut store, handler_component, &linker)
        .context("Failed to instantiate handler")?;

    // Run handler
    match command.wasi_cli_run().call_run(&mut store) {
        Ok(Ok(())) => Ok(format!(
            "{{\"status\": \"ok\", \"path\": \"{}\"}}",
            api_path
        )),
        Ok(Err(())) => Err(anyhow::anyhow!("Handler returned error")),
        Err(e) => Err(anyhow::anyhow!("Handler execution failed: {:?}", e)),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== HTTP Cache Server ===");
    println!();

    // Initialize wasmtime engine
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // Path to WASM handler
    let handler_path = "../../../target/wasm32-wasip2/debug/http-cache-handler.wasm";

    // Load handler component
    println!("Loading handler: {}", handler_path);
    let handler_component =
        Component::from_file(&engine, handler_path).context("Failed to load handler component")?;

    // Create initial VFS state and extract shared VFS (no WASM adapter needed)
    println!("Initializing VFS...");
    let initial_vfs_state =
        vfs_host::VfsHostState::new().context("Failed to create VfsHostState")?;
    let shared_vfs = initial_vfs_state.get_shared_vfs();

    // Create app state
    let state = Arc::new(AppState {
        engine,
        handler_component,
        shared_vfs,
    });

    // Create router
    let app = Router::new()
        .route("/api/*path", get(handle_api_request))
        .route("/health", get(|| async { "OK" }))
        .with_state(state);

    // Start server
    let addr = "0.0.0.0:8080";
    println!();
    println!("Server listening on http://{}", addr);
    println!();
    println!("Try:");
    println!("  curl http://localhost:8080/api/users");
    println!("  curl http://localhost:8080/api/products");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
