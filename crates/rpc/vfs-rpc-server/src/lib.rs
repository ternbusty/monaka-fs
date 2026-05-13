//! VFS RPC Server with S3 Persistence.
//!
//! A WebAssembly component that exposes the `fs-core` filesystem over TCP
//! sockets. Multiple clients can connect and share the same in-memory
//! filesystem. Optional S3 persistence is implemented in `vfs-sync-adapter`.
//!
//! This crate is intentionally a thin shell:
//! - it owns the WASI imports (sockets / I/O / random / clocks);
//! - it runs the TCP accept + per-client read loop;
//! - it generates per-connection UUID v4 session ids from
//!   `wasi:random/random`.
//!
//! The request-handling logic lives in [`vfs_rpc_server_core`], a plain
//! Rust library that builds (and is unit-tested) outside the cdylib.

#![cfg_attr(not(test), no_main)]

use std::rc::Rc;

use prost::Message;
use vfs_rpc_protocol::{
    from_proto_request, to_proto_response, ErrorCode, Response, RpcRequest as ProtoRpcRequest,
};
use vfs_rpc_server_core::{init_server, ServerContext};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "vfs-rpc-server",
    path: "../../../wit",
    generate_all,
});

use wasi::clocks::monotonic_clock::subscribe_duration;
use wasi::io::poll::poll;
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

// Simple logger for WASM compatibility
struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        if cfg!(debug_assertions) {
            true
        } else {
            metadata.level() <= log::Level::Info
        }
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: SimpleLogger = SimpleLogger;

fn init_logger() {
    log::set_logger(&LOGGER).ok();
    if cfg!(debug_assertions) {
        log::set_max_level(log::LevelFilter::Trace);
    } else {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Result of trying to read a message from the socket.
enum ReadResult {
    /// Successfully read a message.
    Message(Vec<u8>),
    /// Client disconnected.
    Disconnected,
}

/// Timeout for polling (100ms in nanoseconds). Short timeout allows other
/// tasks to run (accept loop, other client handlers).
const POLL_TIMEOUT_NS: u64 = 100_000_000;

/// Generate a unique session ID formatted as a UUID v4.
///
/// 16 random bytes come from `wasi:random/random` in WASI builds and from
/// a non-cryptographic host fallback when this file is compiled for unit
/// tests. We avoid the `uuid` crate's built-in `v4` feature because its
/// transitive `getrandom` dependency pulls in `wasi:http/types@0.2.9`,
/// which the component linker cannot merge with the 0.2.6 interfaces our
/// world imports.
fn generate_session_id() -> String {
    uuid::Builder::from_random_bytes(random_uuid_bytes())
        .into_uuid()
        .simple()
        .to_string()
}

#[cfg(target_family = "wasm")]
fn random_uuid_bytes() -> [u8; 16] {
    let bytes = wasi::random::random::get_random_bytes(16);
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    out
}

#[cfg(not(target_family = "wasm"))]
fn random_uuid_bytes() -> [u8; 16] {
    // Host fallback for `cargo test`. The session ID is only used to label
    // log lines, so deterministic-but-distinct bytes derived from the wall
    // clock are good enough — we are not relying on cryptographic strength
    // here. Production builds (wasm32-wasip2) always take the path above.
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&(nanos as u64).to_le_bytes());
    out[8..].copy_from_slice(&seq.to_le_bytes());
    out
}

/// Read a complete length-prefixed message from `stream` (blocking, no
/// timeout). Used after initial data has already been detected.
fn read_message_blocking(stream: &InputStream, first_bytes: Vec<u8>) -> ReadResult {
    let mut len_buf = first_bytes;

    // Read remaining bytes of the 4-byte length prefix
    while len_buf.len() < 4 {
        match stream.blocking_read(4 - len_buf.len() as u64) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    let pollable = stream.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                len_buf.extend_from_slice(&bytes);
            }
            Err(e) => {
                if matches!(e, wasi::io::streams::StreamError::Closed) {
                    return ReadResult::Disconnected;
                }
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }

    let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;

    // Read message body
    let mut data = Vec::with_capacity(len as usize);
    while (data.len() as u64) < len {
        match stream.blocking_read(len - data.len() as u64) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    let pollable = stream.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                data.extend_from_slice(&bytes);
            }
            Err(e) => {
                if matches!(e, wasi::io::streams::StreamError::Closed) {
                    return ReadResult::Disconnected;
                }
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }

    ReadResult::Message(data)
}

/// Write a length-prefixed message to `stream`.
fn write_message(stream: &OutputStream, data: &[u8]) -> bool {
    let len = data.len() as u32;
    let len_bytes = len.to_be_bytes();

    let mut payload = Vec::with_capacity(4 + data.len());
    payload.extend_from_slice(&len_bytes);
    payload.extend_from_slice(data);

    let mut offset = 0;
    while offset < payload.len() {
        let available = match stream.check_write() {
            Ok(n) => n as usize,
            Err(_) => {
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        };
        if available == 0 {
            let pollable = stream.subscribe();
            poll(&[&pollable]);
            continue;
        }
        let end = std::cmp::min(offset + available, payload.len());
        if stream.write(&payload[offset..end]).is_err() {
            return false;
        }
        offset = end;
    }

    loop {
        match stream.blocking_flush() {
            Ok(()) => break,
            Err(_) => {
                let pollable = stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
        }
    }
    true
}

/// Client resources with an explicit drop order: streams first, socket
/// last (so the socket outlives its child streams).
struct ClientResources {
    input: InputStream,
    output: OutputStream,
    #[allow(dead_code)]
    socket: wasi::sockets::tcp::TcpSocket,
}

/// Per-client state for the unified poll loop.
struct Client {
    resources: ClientResources,
    session_id: Option<String>,
}

/// Accept a new client connection and add it to the `clients` list.
fn accept_new_client(socket: &wasi::sockets::tcp::TcpSocket, clients: &mut Vec<Client>) {
    match socket.accept() {
        Ok((client_socket, input, output)) => {
            const TCP_BUF_SIZE: u64 = 4 * 1024 * 1024;
            let _ = client_socket.set_receive_buffer_size(TCP_BUF_SIZE);
            let _ = client_socket.set_send_buffer_size(TCP_BUF_SIZE);

            log::info!("Client connected (total: {})", clients.len() + 1);
            clients.push(Client {
                resources: ClientResources {
                    input,
                    output,
                    socket: client_socket,
                },
                session_id: None,
            });
        }
        Err(wasi::sockets::network::ErrorCode::WouldBlock) => {}
        Err(e) => log::error!("Failed to accept connection: {:?}", e),
    }
}

enum HandleResult {
    /// Request processed successfully, client still connected.
    Ok,
    /// Client disconnected.
    Disconnected,
}

/// Handle one request from a client. Returns whether the client disconnected.
async fn handle_one_request(client: &mut Client, ctx: &ServerContext) -> HandleResult {
    // Read first byte (poll already confirmed data is available)
    let first_byte = match client.resources.input.blocking_read(1) {
        Ok(bytes) if bytes.is_empty() => return HandleResult::Disconnected,
        Ok(bytes) => bytes,
        Err(wasi::io::streams::StreamError::Closed) => return HandleResult::Disconnected,
        Err(_) => return HandleResult::Disconnected,
    };

    // Read the rest of the message
    let request_bytes = match read_message_blocking(&client.resources.input, first_byte.to_vec()) {
        ReadResult::Message(bytes) => bytes,
        ReadResult::Disconnected => return HandleResult::Disconnected,
    };

    // Parse request protobuf
    let proto_request = match ProtoRpcRequest::decode(&request_bytes[..]) {
        Ok(req) => req,
        Err(e) => {
            log::error!("Failed to decode protobuf request: {}", e);
            let response = Response::Error {
                code: ErrorCode::SerializationError,
                message: "Failed to decode protobuf request".to_string(),
            };
            let response_bytes = to_proto_response(response).encode_to_vec();
            write_message(&client.resources.output, &response_bytes);
            return HandleResult::Ok;
        }
    };

    // Convert to internal request type
    let rpc_request = match from_proto_request(proto_request) {
        Ok(req) => req,
        Err(e) => {
            log::error!("Failed to convert request: {}", e);
            let response = Response::Error {
                code: ErrorCode::SerializationError,
                message: e.to_string(),
            };
            let response_bytes = to_proto_response(response).encode_to_vec();
            write_message(&client.resources.output, &response_bytes);
            return HandleResult::Ok;
        }
    };

    // Pre-generate a session id in case this request is `Connect`. The
    // core handler ignores it otherwise.
    let new_session_id = generate_session_id();

    let response = ctx
        .handle_request(
            rpc_request.request,
            client.session_id.clone(),
            new_session_id,
        )
        .await;

    // Track session ID from Connect response.
    if let Response::Connected {
        session_id: ref new_session_id,
        ..
    } = response
    {
        client.session_id = Some(new_session_id.clone());
    }

    // Serialize and send response
    let proto_response = to_proto_response(response);
    let response_bytes = proto_response.encode_to_vec();

    if !write_message(&client.resources.output, &response_bytes) {
        log::info!(
            "Client disconnected (write error, session: {:?})",
            client.session_id
        );
        return HandleResult::Disconnected;
    }

    // Check if we need to sync to S3
    #[cfg(feature = "s3-sync")]
    if let Some(ref sync) = ctx.sync_manager {
        sync.maybe_sync().await;
    }

    HandleResult::Ok
}

/// Main entry point.
#[no_mangle]
pub extern "C" fn _start() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
        .block_on(async_main());
}

async fn async_main() {
    init_logger();
    log::info!("VFS RPC Server starting...");

    let ctx = Rc::new(init_server().await);

    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4).expect("Failed to create TCP socket");

    let bind_addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port: 9000,
        address: (127, 0, 0, 1),
    });

    socket
        .start_bind(&network, bind_addr)
        .expect("Failed to start bind");
    socket.finish_bind().expect("Failed to finish bind");

    log::info!("Socket bound to 127.0.0.1:9000");

    socket.start_listen().expect("Failed to start listen");
    socket.finish_listen().expect("Failed to finish listen");

    log::info!("VFS RPC Server listening on 127.0.0.1:9000");
    log::info!("Protocol version: {}", vfs_rpc_protocol::PROTOCOL_VERSION);
    log::info!("Waiting for connections...");

    // Unified poll loop: accept socket + all client input streams + timeout
    // polled together. No spawn_local/yield_now needed.
    let mut clients: Vec<Client> = Vec::new();

    loop {
        // Collect ready indices in a block so pollables are dropped before
        // we remove any clients. Pollables are child resources of the
        // input/output streams, so the streams must outlive them.
        let (ready_indices, accept_idx, timeout_idx) = {
            let accept_pollable = socket.subscribe();
            let client_pollables: Vec<_> = clients
                .iter()
                .map(|c| c.resources.input.subscribe())
                .collect();
            let timeout_pollable = subscribe_duration(POLL_TIMEOUT_NS);

            let mut all_pollables: Vec<&_> = Vec::with_capacity(2 + clients.len());
            all_pollables.push(&accept_pollable);
            for p in &client_pollables {
                all_pollables.push(p);
            }
            all_pollables.push(&timeout_pollable);

            let ready = poll(&all_pollables);
            let accept_idx = 0u32;
            let timeout_idx = (1 + clients.len()) as u32;
            (ready, accept_idx, timeout_idx)
        };

        let mut to_remove: Vec<usize> = Vec::new();

        for idx in ready_indices {
            if idx == accept_idx {
                accept_new_client(&socket, &mut clients);
            } else if idx == timeout_idx {
                // Periodic S3 sync
                #[cfg(feature = "s3-sync")]
                if let Some(ref sync) = ctx.sync_manager {
                    sync.maybe_sync().await;
                }
            } else {
                let client_idx = (idx - 1) as usize;
                if client_idx < clients.len() {
                    match handle_one_request(&mut clients[client_idx], &ctx).await {
                        HandleResult::Disconnected => {
                            log::info!(
                                "Client disconnected (session: {:?})",
                                clients[client_idx].session_id
                            );
                            #[cfg(feature = "s3-sync")]
                            if let Some(ref sync) = ctx.sync_manager {
                                if sync.pending_count() > 0 {
                                    if let Err(e) = sync.force_flush().await {
                                        log::error!("[sync] Failed to flush: {}", e);
                                    }
                                }
                            }
                            to_remove.push(client_idx);
                        }
                        HandleResult::Ok => {}
                    }
                }
            }
        }

        // Remove disconnected clients (reverse order to preserve indices)
        to_remove.sort_unstable();
        for idx in to_remove.into_iter().rev() {
            clients.swap_remove(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn session_id_is_uuid_simple_form() {
        let id = generate_session_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Best-effort collision check. UUID v4 has 122 bits of entropy, so
    /// even ~10^9 generations should never collide; a hit here almost
    /// certainly means the RNG path or formatting has regressed.
    #[test]
    fn session_ids_are_unique_across_many_generations() {
        let n = 5_000;
        let mut seen = HashSet::with_capacity(n);
        for _ in 0..n {
            let id = generate_session_id();
            assert!(seen.insert(id), "collision within {} generations", n);
        }
    }
}
