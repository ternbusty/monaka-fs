//! Direct RPC Demo
//!
//! This example demonstrates how to communicate directly with the VFS RPC server
//! using WASI sockets, without going through the rpc-fs-runner.
//!
//! This approach gives you full control over the RPC protocol, but requires
//! more boilerplate code compared to the transparent std::fs approach.
//!
//! ## Usage
//!
//! ```bash
//! # Start the VFS RPC server
//! wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/vfs_rpc_server.wasm &
//!
//! # Run this demo (note: uses inherit-network, not rpc-fs-runner)
//! wasmtime run -S inherit-network=y ./target/wasm32-wasip2/debug/direct_rpc_demo.wasm
//! ```

#![no_main]
#![allow(warnings)]

use std::io::{Read, Write};
use std::net::TcpStream;

use vfs_rpc_protocol::{Request, Response, RpcRequest, PROTOCOL_VERSION};

/// Send a length-prefixed message
fn send_message(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(data)?;
    stream.flush()?;
    Ok(())
}

/// Receive a length-prefixed message
fn receive_message(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut data = vec![0u8; len];
    stream.read_exact(&mut data)?;
    Ok(data)
}

/// Send a request and receive a response
fn call(
    stream: &mut TcpStream,
    session_id: Option<String>,
    request: &Request,
) -> std::io::Result<Response> {
    let rpc_request = RpcRequest {
        session_id,
        request: request.clone(),
    };
    let request_json = serde_json::to_vec(&rpc_request)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    send_message(stream, &request_json)?;

    let response_bytes = receive_message(stream)?;
    let response: Response = serde_json::from_slice(&response_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(response)
}

#[no_mangle]
pub extern "C" fn _start() {
    println!("=== Direct RPC Demo ===");
    println!("This demo communicates directly with the VFS RPC server");
    println!("using WASI sockets (no rpc-fs-runner needed).\n");

    // Connect to VFS RPC server
    println!("Connecting to VFS server at 127.0.0.1:9000...");
    let mut stream = match TcpStream::connect("127.0.0.1:9000") {
        Ok(s) => {
            println!("  Connected!");
            s
        }
        Err(e) => {
            eprintln!("  Failed to connect: {}", e);
            eprintln!("  Make sure vfs_rpc_server.wasm is running!");
            return;
        }
    };

    // Handshake (session_id is None for Connect request)
    println!("\nSending handshake...");
    let connect_request = Request::Connect {
        version: PROTOCOL_VERSION,
    };
    let session_id = match call(&mut stream, None, &connect_request) {
        Ok(Response::Connected {
            session_id,
            version,
        }) => {
            println!(
                "  Connected! Session ID: {}, Protocol version: {}",
                session_id, version
            );
            session_id
        }
        Ok(other) => {
            eprintln!("  Unexpected response: {:?}", other);
            return;
        }
        Err(e) => {
            eprintln!("  Handshake failed: {}", e);
            return;
        }
    };

    // Use session_id for all subsequent requests
    let sid = session_id;

    // Create directory
    println!("\nCreating directory /demo...");
    let mkdir_request = Request::Mkdir {
        path: "/demo".to_string(),
    };
    match call(&mut stream, Some(sid.clone()), &mkdir_request) {
        Ok(Response::Ok) => println!("  Directory created!"),
        Ok(Response::Error { code, message }) => {
            println!("  Directory creation result: {:?} - {}", code, message);
        }
        Ok(other) => eprintln!("  Unexpected response: {:?}", other),
        Err(e) => eprintln!("  Request failed: {}", e),
    }

    // Write file
    println!("\nWriting file /demo/hello.txt...");
    let content = b"Hello from Direct RPC Demo!";

    // First, open the file for writing (O_CREAT | O_WRONLY | O_TRUNC = 0x241)
    let open_request = Request::OpenPath {
        path: "/demo/hello.txt".to_string(),
        flags: 0x241, // O_CREAT | O_WRONLY | O_TRUNC
    };
    let fd = match call(&mut stream, Some(sid.clone()), &open_request) {
        Ok(Response::Fd { fd }) => {
            println!("  Opened file, fd={}", fd);
            fd
        }
        Ok(Response::Error { code, message }) => {
            eprintln!("  Open failed: {:?} - {}", code, message);
            return;
        }
        Ok(other) => {
            eprintln!("  Unexpected response: {:?}", other);
            return;
        }
        Err(e) => {
            eprintln!("  Request failed: {}", e);
            return;
        }
    };

    // Write data
    let write_request = Request::Write {
        fd,
        data: content.to_vec(),
    };
    match call(&mut stream, Some(sid.clone()), &write_request) {
        Ok(Response::Written { count }) => {
            println!("  Wrote {} bytes", count);
        }
        Ok(Response::Error { code, message }) => {
            eprintln!("  Write failed: {:?} - {}", code, message);
            return;
        }
        Ok(other) => eprintln!("  Unexpected response: {:?}", other),
        Err(e) => eprintln!("  Request failed: {}", e),
    }

    // Close file
    let close_request = Request::Close { fd };
    let _ = call(&mut stream, Some(sid.clone()), &close_request);

    // Read file back
    println!("\nReading file /demo/hello.txt...");

    // Open for reading (O_RDONLY = 0)
    let open_request = Request::OpenPath {
        path: "/demo/hello.txt".to_string(),
        flags: 0x00, // O_RDONLY
    };
    let fd = match call(&mut stream, Some(sid.clone()), &open_request) {
        Ok(Response::Fd { fd }) => {
            println!("  Opened file, fd={}", fd);
            fd
        }
        Ok(Response::Error { code, message }) => {
            eprintln!("  Open failed: {:?} - {}", code, message);
            return;
        }
        Ok(other) => {
            eprintln!("  Unexpected response: {:?}", other);
            return;
        }
        Err(e) => {
            eprintln!("  Request failed: {}", e);
            return;
        }
    };

    // Read data
    let read_request = Request::Read { fd, length: 1024 };
    match call(&mut stream, Some(sid.clone()), &read_request) {
        Ok(Response::Data { bytes }) => {
            let text = String::from_utf8_lossy(&bytes);
            println!("  Read {} bytes: \"{}\"", bytes.len(), text);
        }
        Ok(Response::Error { code, message }) => {
            eprintln!("  Read failed: {:?} - {}", code, message);
        }
        Ok(other) => eprintln!("  Unexpected response: {:?}", other),
        Err(e) => eprintln!("  Request failed: {}", e),
    }

    // Close file
    let close_request = Request::Close { fd };
    let _ = call(&mut stream, Some(sid.clone()), &close_request);

    // Get file stats
    println!("\nGetting file stats for /demo/hello.txt...");
    let stat_request = Request::Stat {
        path: "/demo/hello.txt".to_string(),
    };
    match call(&mut stream, Some(sid.clone()), &stat_request) {
        Ok(Response::Metadata { metadata }) => {
            println!("  Size: {} bytes", metadata.size);
            println!("  Is directory: {}", metadata.is_dir);
        }
        Ok(Response::Error { code, message }) => {
            eprintln!("  Stat failed: {:?} - {}", code, message);
        }
        Ok(other) => eprintln!("  Unexpected response: {:?}", other),
        Err(e) => eprintln!("  Request failed: {}", e),
    }

    // List directory
    println!("\nListing directory /demo...");
    let readdir_request = Request::Readdir {
        path: "/demo".to_string(),
    };
    match call(&mut stream, Some(sid.clone()), &readdir_request) {
        Ok(Response::DirEntries { entries }) => {
            println!("  Found {} entries:", entries.len());
            for entry in entries {
                let type_str = if entry.is_dir { "[DIR]" } else { "[FILE]" };
                println!("    {} {}", type_str, entry.name);
            }
        }
        Ok(Response::Error { code, message }) => {
            eprintln!("  Readdir failed: {:?} - {}", code, message);
        }
        Ok(other) => eprintln!("  Unexpected response: {:?}", other),
        Err(e) => eprintln!("  Request failed: {}", e),
    }

    println!("\n=== Direct RPC Demo completed ===");
}
