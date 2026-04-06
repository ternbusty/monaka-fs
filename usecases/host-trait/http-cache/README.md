# HTTP Cache Demo

Real HTTP server (axum) that routes requests to WASM handlers sharing an in-memory VFS cache via host-trait (`vfs-host`).

**Deployment method**: Host Trait (`vfs-host` crate — `cargo add vfs-host`)

> This use case demonstrates the Host Trait method, where a native Rust program hosts WASM instances sharing a single VFS. The `monaka` CLI is not used here; instead, the host program (`http-cache-server`) links against `vfs-host` directly.

```
HTTP Request --> axum server --> spawn_blocking --> WASM handler
                                                      |
                                              shared VFS (Arc<Mutex>)
                                                /cache/*.cache
```

## Build

```bash
# From repository root:
cargo build -p http-cache-handler --target wasm32-wasip2
cargo build -p http-cache-server
```

## Run

```bash
cargo run -p http-cache-server
```

In another terminal:

```bash
curl http://localhost:8080/api/users      # Cache miss
curl http://localhost:8080/api/products   # Cache miss
curl http://localhost:8080/api/users      # Cache hit
```

## Expected Output

Server logs:

```
[SERVER] Handling request: /api/users
[CACHE MISS] /api/users
[API FETCH] /api/users (mock)
[CACHE WRITE] /cache/api_users.cache

[SERVER] Handling request: /api/products
[CACHE MISS] /api/products
[API FETCH] /api/products (mock)
[CACHE WRITE] /cache/api_products.cache

[SERVER] Handling request: /api/users
[CACHE HIT] /api/users (expires in 295s)
```
