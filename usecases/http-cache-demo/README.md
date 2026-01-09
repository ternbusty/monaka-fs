# HTTP Cache Demo

Demonstrates VFS cache sharing between multiple WASM instances using a Fermyon Spin-like architecture with a real HTTP server.

## Running the Demo

```bash
make run-usecase-http-cache
```

Then open another terminal and test with:

```bash
curl http://localhost:8080/api/users
curl http://localhost:8080/api/products
curl http://localhost:8080/api/users   # Cache hit
```

## Architecture

```
                  ┌─────────────────────────────────────┐
                  │        HTTP Cache Server            │
                  │         (axum + tokio)              │
                  ├─────────────────────────────────────┤
   HTTP Request   │                                     │
   ────────────>  │   spawn_blocking ──> WASM Handler   │
                  │         │                    │      │
                  │         └──── shared_vfs ────┘      │
                  │              (Arc<Mutex>)           │
                  └─────────────────────────────────────┘
```

## Cache File Format

```json
{
  "cached_at": 1704307200,
  "ttl_seconds": 300,
  "data": { ... }
}
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
