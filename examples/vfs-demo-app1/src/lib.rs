//! VFS Demo App 1 - File Writer
//!
//! Connects to VFS RPC server and creates a file with some content.

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
    println!("=== VFS Demo App 1: File Writer ===");

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
    println!("Sending Connect request...");
    let connect_req = Request::Connect {
        version: PROTOCOL_VERSION,
    };
    send_request(&output, &connect_req);
    println!("Connect request sent, waiting for response...");
    let response: Response = receive_response(&input);
    println!("Received response!");

    match response {
        Response::Connected { session_id, .. } => {
            println!("Session ID: {}", session_id);
        }
        _ => {
            eprintln!("Unexpected response");
            return;
        }
    }

    // Create /shared directory first
    println!("\nCreating directory: /shared");
    let mkdir_req = Request::Mkdir {
        path: "/shared".to_string(),
    };
    send_request(&output, &mkdir_req);
    match receive_response(&input) {
        Response::Ok => println!("Directory created successfully"),
        Response::Error { message, .. } => println!("Directory creation: {}", message),
        _ => eprintln!("Unexpected response"),
    }

    // Create and write file
    println!("\nCreating file: /shared/message.txt");
    let open_req = Request::OpenPath {
        path: "/shared/message.txt".to_string(),
        flags: 0x42,
    };
    send_request(&output, &open_req);
    let fd = match receive_response(&input) {
        Response::Fd { fd } => {
            println!("File opened with fd: {}", fd);
            fd
        }
        Response::Error { message, .. } => {
            eprintln!("Failed to open: {}", message);
            return;
        }
        _ => {
            eprintln!("Unexpected response");
            return;
        }
    };

    // Write content
    let message = b"Hello from App1! This file is shared via VFS RPC.";
    println!("Writing {} bytes...", message.len());
    let write_req = Request::Write {
        fd,
        data: message.to_vec(),
    };
    send_request(&output, &write_req);
    match receive_response(&input) {
        Response::Written { count } => println!("Wrote {} bytes", count),
        _ => eprintln!("Write failed"),
    }

    // Close file
    let close_req = Request::Close { fd };
    send_request(&output, &close_req);
    let _ = receive_response(&input);

    println!("\n=== App1 completed successfully ===");
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
