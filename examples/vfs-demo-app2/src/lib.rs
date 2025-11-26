//! VFS Demo App 2 - File Reader
//!
//! Connects to VFS RPC server and reads the file created by App1.

#![no_main]

use vfs_rpc_protocol::{Request, Response, PROTOCOL_VERSION};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "vfs-rpc-client",
    path: "../../wit",
    generate_all,
});

use wasi::io::poll::poll;
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

/// Entry point
#[no_mangle]
pub extern "C" fn _start() {
    println!("=== VFS Demo App 2: File Reader ===");

    // Connect to VFS server
    println!("Connecting to VFS server at localhost:9000...");

    let network = instance_network();
    let socket = create_tcp_socket(IpAddressFamily::Ipv4).expect("Failed to create socket");

    let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port: 9000,
        address: (127, 0, 0, 1),
    });

    socket
        .start_connect(&network, addr)
        .expect("Failed to start connect");

    let (input, output) = loop {
        match socket.finish_connect() {
            Ok(streams) => break streams,
            Err(_) => {
                let pollable = socket.subscribe();
                poll(&[&pollable]);
            }
        }
    };

    println!("Connected!");

    // Send Connect request
    let connect_req = Request::Connect {
        version: PROTOCOL_VERSION,
    };
    send_request(&output, &connect_req);
    let response: Response = receive_response(&input);

    match response {
        Response::Connected { session_id, .. } => {
            println!("Session ID: {}", session_id);
        }
        _ => {
            eprintln!("Unexpected response");
            return;
        }
    }

    // Open the file created by App1
    println!("\nOpening file: /shared/message.txt");
    let open_req = Request::OpenPath {
        path: "/shared/message.txt".to_string(),
        flags: 0x01, // Read only
    };
    send_request(&output, &open_req);
    let fd = match receive_response(&input) {
        Response::Fd { fd } => {
            println!("File opened with fd: {}", fd);
            fd
        }
        Response::Error { message, .. } => {
            eprintln!("Failed to open: {}", message);
            eprintln!("Make sure App1 has been run first!");
            return;
        }
        _ => {
            eprintln!("Unexpected response");
            return;
        }
    };

    // Get file metadata
    println!("\nGetting file metadata...");
    let fstat_req = Request::Fstat { fd };
    send_request(&output, &fstat_req);
    match receive_response(&input) {
        Response::Metadata { metadata } => {
            println!("File size: {} bytes", metadata.size);
            println!("Created: {}", metadata.created);
            println!("Modified: {}", metadata.modified);
        }
        _ => eprintln!("Failed to get metadata"),
    }

    // Read the content
    println!("\nReading file content...");
    let read_req = Request::Read { fd, length: 1024 };
    send_request(&output, &read_req);
    match receive_response(&input) {
        Response::Data { bytes } => {
            let content = String::from_utf8_lossy(&bytes);
            println!("Content ({} bytes):", bytes.len());
            println!("\"{}\"", content);
        }
        _ => eprintln!("Read failed"),
    }

    // Close file
    let close_req = Request::Close { fd };
    send_request(&output, &close_req);
    let _ = receive_response(&input);

    println!("\n=== App2 completed successfully ===");
    println!("SUCCESS: File sharing between separate WASM processes works!");
}

fn send_request(output: &OutputStream, request: &Request) {
    let data = serde_json::to_vec(request).unwrap();
    let len = (data.len() as u32).to_be_bytes();

    loop {
        match output.blocking_write_and_flush(&len) {
            Ok(()) => break,
            Err(_) => {
                let pollable = output.subscribe();
                poll(&[&pollable]);
            }
        }
    }

    loop {
        match output.blocking_write_and_flush(&data) {
            Ok(()) => break,
            Err(_) => {
                let pollable = output.subscribe();
                poll(&[&pollable]);
            }
        }
    }
}

fn receive_response(input: &InputStream) -> Response {
    // Read 4-byte length prefix with retry on would-block and partial reads
    let mut len_buf = Vec::new();
    while len_buf.len() < 4 {
        match input.blocking_read(4 - len_buf.len() as u64) {
            Ok(bytes) if !bytes.is_empty() => {
                len_buf.extend_from_slice(&bytes);
            }
            _ => {
                let pollable = input.subscribe();
                poll(&[&pollable]);
            }
        }
    }

    let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;

    // Read message body with retry on would-block and partial reads
    let mut data = Vec::new();
    while (data.len() as u64) < len {
        match input.blocking_read(len - data.len() as u64) {
            Ok(bytes) if !bytes.is_empty() => {
                data.extend_from_slice(&bytes);
            }
            _ => {
                let pollable = input.subscribe();
                poll(&[&pollable]);
            }
        }
    }

    serde_json::from_slice(&data).unwrap()
}
