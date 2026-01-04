// RPC Adapter: WASI filesystem adapter that makes RPC calls to vfs-rpc-server
//
// This is a component that exports WASI filesystem interfaces
// and delegates to vfs-rpc-server via TCP RPC calls.
//
// Design: Uses persistent TCP connection with WASI poll for efficient I/O.
// Socket is kept in PersistentConnection to prevent premature drop.
// subscribe() creates child Pollables, but they are dropped within each loop iteration.

#![no_main]
#![allow(warnings)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Once;

use vfs_rpc_protocol::{Request, Response, RpcRequest, PROTOCOL_VERSION};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "rpc-adapter",
    path: "../../../wit",
    generate_all,
});

// Re-export for convenience
use exports::wasi::filesystem::types::{
    Descriptor, DescriptorBorrow, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    DirectoryEntryStream, ErrorCode, Filesize, NewTimestamp, OpenFlags, PathFlags,
};

use wasi::io::poll::poll;
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::TcpSocket;
use wasi::sockets::tcp_create_socket::create_tcp_socket;

// Persistent RPC connection: holds socket and streams globally.
// Socket must be kept alive to prevent "resource has children" error when it would be dropped.
// subscribe() creates child Pollables, but they are dropped within each loop iteration.

static CONN_INIT: Once = Once::new();
static mut RPC_CONNECTION: Option<PersistentConnection> = None;

struct PersistentConnection {
    // Socket is kept alive to prevent premature drop (streams are children of socket)
    #[allow(dead_code)]
    socket: TcpSocket,
    input_stream: InputStream,
    output_stream: OutputStream,
    session_id: String,
}

impl PersistentConnection {
    fn connect() -> Result<Self, ErrorCode> {
        eprintln!("[RPC-ADAPTER] PersistentConnection::connect: starting");

        // Create TCP socket
        let network = instance_network();
        let socket = create_tcp_socket(IpAddressFamily::Ipv4).map_err(|e| {
            eprintln!("[RPC-ADAPTER] Failed to create socket: {:?}", e);
            ErrorCode::Io
        })?;

        // Connect to localhost:9000
        let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port: 9000,
            address: (127, 0, 0, 1),
        });

        socket.start_connect(&network, addr).map_err(|e| {
            eprintln!("[RPC-ADAPTER] Failed to start connect: {:?}", e);
            ErrorCode::Io
        })?;

        eprintln!("[RPC-ADAPTER] PersistentConnection::connect: waiting for connection");

        // Wait for connection to complete using poll() for efficient waiting
        let (mut input_stream, mut output_stream) = loop {
            match socket.finish_connect() {
                Ok(streams) => break streams,
                Err(wasi::sockets::network::ErrorCode::WouldBlock) => {
                    // Use subscribe() + poll() for efficient waiting
                    let pollable = socket.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                Err(e) => {
                    eprintln!("[RPC-ADAPTER] Failed to finish connect: {:?}", e);
                    return Err(ErrorCode::Io);
                }
            }
        };

        eprintln!("[RPC-ADAPTER] PersistentConnection::connect: connected, sending handshake");

        // Do handshake
        // session_id is None for the Connect request
        Self::send_raw(
            &mut output_stream,
            None,
            &Request::Connect {
                version: PROTOCOL_VERSION,
            },
        )?;

        eprintln!(
            "[RPC-ADAPTER] PersistentConnection::connect: handshake sent, waiting for response"
        );

        match Self::receive_raw(&mut input_stream) {
            Ok(Response::Connected { session_id, .. }) => {
                eprintln!(
                    "[RPC-ADAPTER] PersistentConnection::connect: connected, session_id={}",
                    session_id
                );
                Ok(Self {
                    socket,
                    input_stream,
                    output_stream,
                    session_id,
                })
            }
            Ok(other) => {
                eprintln!(
                    "[RPC-ADAPTER] PersistentConnection::connect: unexpected response: {:?}",
                    other
                );
                Err(ErrorCode::Io)
            }
            Err(e) => {
                eprintln!(
                    "[RPC-ADAPTER] PersistentConnection::connect: handshake failed: {:?}",
                    e
                );
                Err(e)
            }
        }
    }

    fn send_raw(
        output_stream: &mut OutputStream,
        session_id: Option<String>,
        request: &Request,
    ) -> Result<(), ErrorCode> {
        let rpc_request = RpcRequest {
            session_id,
            request: request.clone(),
        };
        let data = serde_json::to_vec(&rpc_request).map_err(|e| {
            eprintln!("[RPC-ADAPTER] send_raw: JSON serialize error: {:?}", e);
            ErrorCode::Io
        })?;
        let len = (data.len() as u32).to_be_bytes();

        eprintln!(
            "[RPC-ADAPTER] send_raw: sending {} bytes ({})",
            data.len(),
            String::from_utf8_lossy(&data[..std::cmp::min(100, data.len())])
        );

        // Write length prefix (blocking, NO subscribe)
        output_stream.blocking_write_and_flush(&len).map_err(|e| {
            eprintln!("[RPC-ADAPTER] send_raw: write length prefix error: {:?}", e);
            ErrorCode::Io
        })?;

        // Write JSON payload (blocking, NO subscribe)
        output_stream.blocking_write_and_flush(&data).map_err(|e| {
            eprintln!("[RPC-ADAPTER] send_raw: write body error: {:?}", e);
            ErrorCode::Io
        })?;

        eprintln!("[RPC-ADAPTER] send_raw: sent successfully");
        Ok(())
    }

    fn receive_raw(input_stream: &mut InputStream) -> Result<Response, ErrorCode> {
        eprintln!("[RPC-ADAPTER] receive_raw: reading length prefix...");
        // Read 4-byte length prefix using poll() for efficient waiting
        let mut len_buf = Vec::new();

        while len_buf.len() < 4 {
            let remaining = 4 - len_buf.len() as u64;
            let bytes = match input_stream.blocking_read(remaining) {
                Ok(b) => b,
                Err(wasi::io::streams::StreamError::Closed) => {
                    eprintln!("[RPC-ADAPTER] receive_raw: stream closed");
                    return Err(ErrorCode::Io);
                }
                Err(e) => {
                    eprintln!("[RPC-ADAPTER] receive_raw: blocking_read error: {:?}", e);
                    return Err(ErrorCode::Io);
                }
            };
            if bytes.is_empty() {
                // Use subscribe() + poll() for efficient waiting
                let pollable = input_stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
            len_buf.extend_from_slice(&bytes);
        }

        let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;
        eprintln!("[RPC-ADAPTER] receive_raw: message length is {} bytes", len);

        // Read message body using poll() for efficient waiting
        let mut data = Vec::new();

        while (data.len() as u64) < len {
            let remaining = len - data.len() as u64;
            let bytes = match input_stream.blocking_read(remaining) {
                Ok(b) => b,
                Err(wasi::io::streams::StreamError::Closed) => {
                    eprintln!("[RPC-ADAPTER] receive_raw: stream closed during body read");
                    return Err(ErrorCode::Io);
                }
                Err(e) => {
                    eprintln!(
                        "[RPC-ADAPTER] receive_raw: blocking_read body error: {:?}",
                        e
                    );
                    return Err(ErrorCode::Io);
                }
            };
            if bytes.is_empty() {
                // Use subscribe() + poll() for efficient waiting
                let pollable = input_stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
            data.extend_from_slice(&bytes);
        }

        eprintln!(
            "[RPC-ADAPTER] receive_raw: parsing {} bytes of JSON",
            data.len()
        );
        serde_json::from_slice(&data).map_err(|e| {
            eprintln!("[RPC-ADAPTER] receive_raw: JSON parse error: {:?}", e);
            ErrorCode::Io
        })
    }

    fn send(&mut self, request: &Request) -> Result<(), ErrorCode> {
        eprintln!(
            "[RPC-ADAPTER] send: calling send_raw with session_id={}",
            self.session_id
        );
        Self::send_raw(
            &mut self.output_stream,
            Some(self.session_id.clone()),
            request,
        )
    }

    fn receive(&mut self) -> Result<Response, ErrorCode> {
        eprintln!("[RPC-ADAPTER] receive: calling receive_raw");
        Self::receive_raw(&mut self.input_stream)
    }

    fn call(&mut self, request: &Request) -> Result<Response, ErrorCode> {
        eprintln!("[RPC-ADAPTER] call: sending request");
        self.send(request)?;
        eprintln!("[RPC-ADAPTER] call: request sent, receiving response");
        self.receive()
    }
}

// Get or initialize the persistent connection
fn with_connection<F, R>(f: F) -> Result<R, ErrorCode>
where
    F: FnOnce(&mut PersistentConnection) -> Result<R, ErrorCode>,
{
    unsafe {
        CONN_INIT.call_once(|| {
            match PersistentConnection::connect() {
                Ok(conn) => {
                    RPC_CONNECTION = Some(conn);
                }
                Err(e) => {
                    eprintln!(
                        "[RPC-ADAPTER] Failed to establish persistent connection: {:?}",
                        e
                    );
                    // Connection will be None, subsequent calls will fail
                }
            }
        });

        match RPC_CONNECTION.as_mut() {
            Some(conn) => f(conn),
            None => {
                eprintln!("[RPC-ADAPTER] No connection available");
                Err(ErrorCode::Io)
            }
        }
    }
}

// Helper to make RPC call using persistent connection
fn rpc_call(request: &Request) -> Result<Response, ErrorCode> {
    with_connection(|conn| conn.call(request))
}

// Main RPC adapter state: only stores descriptor mappings, no connection
static INIT: Once = Once::new();
static mut RPC_STATE: Option<RpcState> = None;

struct RpcState {
    // Map descriptor handle to server FD
    descriptor_to_fd: RefCell<BTreeMap<u32, u32>>,
    // Map server FD to descriptor handle
    fd_to_descriptor: RefCell<BTreeMap<u32, u32>>,
    next_descriptor: RefCell<u32>,
}

impl RpcState {
    fn new() -> Self {
        let state = Self {
            descriptor_to_fd: RefCell::new(BTreeMap::new()),
            fd_to_descriptor: RefCell::new(BTreeMap::new()),
            next_descriptor: RefCell::new(1),
        };

        // Register root directory as descriptor 0, server FD 0
        state.descriptor_to_fd.borrow_mut().insert(0, 0);
        state.fd_to_descriptor.borrow_mut().insert(0, 0);

        state
    }

    fn allocate_descriptor(&self, server_fd: u32) -> u32 {
        let desc = *self.next_descriptor.borrow();
        *self.next_descriptor.borrow_mut() += 1;
        self.descriptor_to_fd.borrow_mut().insert(desc, server_fd);
        self.fd_to_descriptor.borrow_mut().insert(server_fd, desc);
        desc
    }

    fn get_server_fd(&self, descriptor: u32) -> Result<u32, ErrorCode> {
        self.descriptor_to_fd
            .borrow()
            .get(&descriptor)
            .copied()
            .ok_or(ErrorCode::BadDescriptor)
    }

    fn release_descriptor(&self, descriptor: u32) {
        if let Some(fd) = self.descriptor_to_fd.borrow_mut().remove(&descriptor) {
            self.fd_to_descriptor.borrow_mut().remove(&fd);
        }
    }
}

// Helper to get or initialize RPC state
fn with_rpc_state<F, R>(f: F) -> R
where
    F: FnOnce(&RpcState) -> R,
{
    unsafe {
        INIT.call_once(|| {
            RPC_STATE = Some(RpcState::new());
        });
        f(RPC_STATE.as_ref().unwrap())
    }
}

// Convert RPC error to WASI error code
fn rpc_error_to_wasi(code: vfs_rpc_protocol::ErrorCode) -> ErrorCode {
    use vfs_rpc_protocol::ErrorCode as RpcError;
    match code {
        RpcError::NotFound => ErrorCode::NoEntry,
        RpcError::NotADirectory => ErrorCode::NotDirectory,
        RpcError::IsADirectory => ErrorCode::IsDirectory,
        RpcError::InvalidArgument => ErrorCode::Invalid,
        RpcError::BadFileDescriptor => ErrorCode::BadDescriptor,
        RpcError::PermissionDenied => ErrorCode::Access,
        RpcError::AlreadyExists => ErrorCode::Exist,
        RpcError::NotEmpty => ErrorCode::NotEmpty,
        _ => ErrorCode::Io,
    }
}

// Convert WASI flags to fs-core flags
fn convert_flags(open_flags: OpenFlags, descriptor_flags: DescriptorFlags) -> u32 {
    let mut flags = 0u32;

    // Access mode
    if descriptor_flags.contains(DescriptorFlags::READ)
        && descriptor_flags.contains(DescriptorFlags::WRITE)
    {
        flags |= 0x02; // O_RDWR
    } else if descriptor_flags.contains(DescriptorFlags::WRITE) {
        flags |= 0x01; // O_WRONLY
    } else {
        flags |= 0x00; // O_RDONLY
    }

    // Open flags
    if open_flags.contains(OpenFlags::CREATE) {
        flags |= 0x40; // O_CREAT
    }
    if open_flags.contains(OpenFlags::TRUNCATE) {
        flags |= 0x200; // O_TRUNC
    }

    flags
}

// Export the preopens interface
export!(RpcAdapter);

struct RpcAdapter;

impl exports::wasi::filesystem::preopens::Guest for RpcAdapter {
    fn get_directories() -> Vec<(Descriptor, String)> {
        eprintln!("[RPC-ADAPTER] get_directories() called");

        // Initialize state (no connection stored, just descriptor mappings)
        let fd = with_rpc_state(|state| state.descriptor_to_fd.borrow().get(&0).copied());

        match fd {
            Some(fd) => {
                eprintln!("[RPC-ADAPTER] State verified: descriptor 0 -> fd {}", fd);
                // Descriptor 0 is mapped to root directory
                let desc = Descriptor::new(DescriptorImpl { handle: 0 });
                eprintln!("[RPC-ADAPTER] Returning descriptor 0 for path /");
                vec![(desc, "/".to_string())]
            }
            None => {
                eprintln!("[RPC-ADAPTER] ERROR: descriptor 0 not mapped");
                // State exists but descriptor 0 not mapped. This shouldn't happen
                vec![]
            }
        }
    }
}

impl exports::wasi::filesystem::types::Guest for RpcAdapter {
    type Descriptor = DescriptorImpl;
    type DirectoryEntryStream = DirectoryEntryStreamImpl;

    fn filesystem_error_code(_err: exports::wasi::io::error::ErrorBorrow<'_>) -> Option<ErrorCode> {
        None
    }
}

// Descriptor resource implementation
struct DescriptorImpl {
    handle: u32,
}

impl exports::wasi::filesystem::types::GuestDescriptor for DescriptorImpl {
    fn read_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::InputStream, ErrorCode> {
        eprintln!(
            "[RPC-ADAPTER] read_via_stream called for handle={}, offset={}",
            self.handle, offset
        );
        // Verify the descriptor is valid
        with_rpc_state(|state| state.get_server_fd(self.handle))?;

        Ok(exports::wasi::filesystem::types::InputStream::new(
            UnifiedInputStream::File(FileInputStream {
                handle: self.handle,
                offset: Cell::new(offset),
            }),
        ))
    }

    fn write_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        eprintln!(
            "[RPC-ADAPTER] write_via_stream called for handle={}, offset={}",
            self.handle, offset
        );
        // Verify the descriptor is valid
        with_rpc_state(|state| state.get_server_fd(self.handle))?;

        Ok(exports::wasi::filesystem::types::OutputStream::new(
            UnifiedOutputStream::File(FileOutputStream {
                handle: self.handle,
                offset: Cell::new(offset),
                append: false,
            }),
        ))
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        // Verify the descriptor is valid
        with_rpc_state(|state| state.get_server_fd(self.handle))?;

        Ok(exports::wasi::filesystem::types::OutputStream::new(
            UnifiedOutputStream::File(FileOutputStream {
                handle: self.handle,
                offset: Cell::new(0),
                append: true,
            }),
        ))
    }

    fn advise(
        &self,
        _offset: Filesize,
        _length: Filesize,
        _advice: exports::wasi::filesystem::types::Advice,
    ) -> Result<(), ErrorCode> {
        Ok(())
    }

    fn sync_data(&self) -> Result<(), ErrorCode> {
        Ok(())
    }

    fn get_flags(&self) -> Result<DescriptorFlags, ErrorCode> {
        Ok(DescriptorFlags::READ | DescriptorFlags::WRITE)
    }

    fn get_type(&self) -> Result<DescriptorType, ErrorCode> {
        if self.handle == 0 {
            return Ok(DescriptorType::Directory);
        }

        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let request = Request::Fstat { fd: server_fd };
        match rpc_call(&request)? {
            Response::Metadata { metadata } => {
                if metadata.is_dir {
                    Ok(DescriptorType::Directory)
                } else {
                    Ok(DescriptorType::RegularFile)
                }
            }
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn set_size(&self, size: Filesize) -> Result<(), ErrorCode> {
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let request = Request::Ftruncate {
            fd: server_fd,
            size,
        };
        match rpc_call(&request)? {
            Response::Ok => Ok(()),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn set_times(
        &self,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn read(&self, length: Filesize, offset: Filesize) -> Result<(Vec<u8>, bool), ErrorCode> {
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        // Use persistent connection to perform seek + read atomically
        with_connection(|conn| {
            // Seek to offset
            let seek_request = Request::Seek {
                fd: server_fd,
                offset: offset as i64,
                whence: 0,
            };
            conn.send(&seek_request)?;

            match conn.receive()? {
                Response::Position { .. } => {}
                Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                _ => return Err(ErrorCode::Io),
            }

            // Read data
            let read_request = Request::Read {
                fd: server_fd,
                length: length as usize,
            };
            conn.send(&read_request)?;

            match conn.receive()? {
                Response::Data { bytes } => {
                    let end_of_stream = bytes.len() < length as usize;
                    Ok((bytes, end_of_stream))
                }
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })
    }

    fn write(&self, buffer: Vec<u8>, offset: Filesize) -> Result<Filesize, ErrorCode> {
        eprintln!(
            "[RPC-ADAPTER] write called for handle={}, buffer.len()={}, offset={}",
            self.handle,
            buffer.len(),
            offset
        );
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        // Use persistent connection to perform seek + write atomically
        with_connection(|conn| {
            // Seek to offset
            let seek_request = Request::Seek {
                fd: server_fd,
                offset: offset as i64,
                whence: 0,
            };
            conn.send(&seek_request)?;

            match conn.receive()? {
                Response::Position { .. } => {}
                Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                _ => return Err(ErrorCode::Io),
            }

            // Write data
            let write_request = Request::Write {
                fd: server_fd,
                data: buffer,
            };
            conn.send(&write_request)?;

            match conn.receive()? {
                Response::Written { count } => Ok(count as Filesize),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })
    }

    fn read_directory(
        &self,
    ) -> Result<exports::wasi::filesystem::types::DirectoryEntryStream, ErrorCode> {
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let request = Request::ReaddirFd { fd: server_fd };
        match rpc_call(&request)? {
            Response::DirEntries { entries } => {
                // Convert RPC entries to WASI entries
                let wasi_entries: Vec<DirectoryEntry> = entries
                    .into_iter()
                    .map(|e| DirectoryEntry {
                        type_: if e.is_dir {
                            DescriptorType::Directory
                        } else {
                            DescriptorType::RegularFile
                        },
                        name: e.name,
                    })
                    .collect();

                Ok(exports::wasi::filesystem::types::DirectoryEntryStream::new(
                    DirectoryEntryStreamImpl {
                        entries: RefCell::new(wasi_entries),
                        index: Cell::new(0),
                    },
                ))
            }
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn sync(&self) -> Result<(), ErrorCode> {
        Ok(())
    }

    fn create_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        // For root directory, use direct path
        let full_path = if self.handle == 0 {
            format!("/{}", path.trim_start_matches('/'))
        } else {
            // Would need to track paths, for now just use relative
            path
        };

        let request = Request::Mkdir { path: full_path };
        match rpc_call(&request)? {
            Response::Ok => Ok(()),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn stat(&self) -> Result<DescriptorStat, ErrorCode> {
        if self.handle == 0 {
            // Root directory
            return Ok(DescriptorStat {
                type_: DescriptorType::Directory,
                link_count: 1,
                size: 0,
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            });
        }

        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let request = Request::Fstat { fd: server_fd };
        match rpc_call(&request)? {
            Response::Metadata { metadata } => Ok(DescriptorStat {
                type_: if metadata.is_dir {
                    DescriptorType::Directory
                } else {
                    DescriptorType::RegularFile
                },
                link_count: 1,
                size: metadata.size as Filesize,
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            }),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn stat_at(&self, _path_flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        let full_path = if self.handle == 0 {
            format!("/{}", path.trim_start_matches('/'))
        } else {
            path
        };

        let request = Request::Stat { path: full_path };
        match rpc_call(&request)? {
            Response::Metadata { metadata } => Ok(DescriptorStat {
                type_: if metadata.is_dir {
                    DescriptorType::Directory
                } else {
                    DescriptorType::RegularFile
                },
                link_count: 1,
                size: metadata.size as Filesize,
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            }),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn set_times_at(
        &self,
        _path_flags: PathFlags,
        _path: String,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn link_at(
        &self,
        _old_path_flags: PathFlags,
        _old_path: String,
        _new_descriptor: DescriptorBorrow<'_>,
        _new_path: String,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn open_at(
        &self,
        _path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        flags: DescriptorFlags,
    ) -> Result<Descriptor, ErrorCode> {
        eprintln!(
            "[RPC-ADAPTER] open_at called: handle={}, path={}",
            self.handle, path
        );

        let full_path = if self.handle == 0 {
            format!("/{}", path.trim_start_matches('/'))
        } else {
            path
        };

        eprintln!("[RPC-ADAPTER] open_at: full_path={}", full_path);
        let fs_flags = convert_flags(open_flags, flags);

        let request = Request::OpenPath {
            path: full_path,
            flags: fs_flags,
        };

        // Make the RPC call first, then allocate descriptor after connection is closed
        let server_fd = match rpc_call(&request)? {
            Response::Fd { fd } => fd,
            Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
            _ => return Err(ErrorCode::Io),
        };

        // Now allocate descriptor (RPC connection is already closed)
        let desc_id = with_rpc_state(|state| state.allocate_descriptor(server_fd));
        Ok(Descriptor::new(DescriptorImpl { handle: desc_id }))
    }

    fn readlink_at(&self, _path: String) -> Result<String, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn remove_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        let full_path = if self.handle == 0 {
            format!("/{}", path.trim_start_matches('/'))
        } else {
            path
        };

        let request = Request::Rmdir { path: full_path };
        match rpc_call(&request)? {
            Response::Ok => Ok(()),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn rename_at(
        &self,
        _old_path: String,
        _new_descriptor: DescriptorBorrow<'_>,
        _new_path: String,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn symlink_at(&self, _old_path: String, _new_path: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn unlink_file_at(&self, path: String) -> Result<(), ErrorCode> {
        let full_path = if self.handle == 0 {
            format!("/{}", path.trim_start_matches('/'))
        } else {
            path
        };

        let request = Request::Unlink { path: full_path };
        match rpc_call(&request)? {
            Response::Ok => Ok(()),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn is_same_object(&self, other: DescriptorBorrow<'_>) -> bool {
        self.handle == other.get::<DescriptorImpl>().handle
    }

    fn metadata_hash(
        &self,
    ) -> Result<exports::wasi::filesystem::types::MetadataHashValue, ErrorCode> {
        Ok(exports::wasi::filesystem::types::MetadataHashValue { lower: 0, upper: 0 })
    }

    fn metadata_hash_at(
        &self,
        _path_flags: PathFlags,
        _path: String,
    ) -> Result<exports::wasi::filesystem::types::MetadataHashValue, ErrorCode> {
        Ok(exports::wasi::filesystem::types::MetadataHashValue { lower: 0, upper: 0 })
    }
}

// Directory entry stream implementation
struct DirectoryEntryStreamImpl {
    entries: RefCell<Vec<DirectoryEntry>>,
    index: Cell<usize>,
}

// File input stream implementation for read_via_stream
struct FileInputStream {
    handle: u32,       // Descriptor handle
    offset: Cell<u64>, // Current read position
}

impl exports::wasi::io::streams::GuestInputStream for FileInputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        self.blocking_read(len)
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle)).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })?;

        let current_offset = self.offset.get();

        let result = with_connection(|conn| {
            // Seek to offset
            let seek_request = Request::Seek {
                fd: server_fd,
                offset: current_offset as i64,
                whence: 0,
            };
            conn.send(&seek_request)?;

            match conn.receive()? {
                Response::Position { .. } => {}
                Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                _ => return Err(ErrorCode::Io),
            }

            // Read data
            let read_request = Request::Read {
                fd: server_fd,
                length: len as usize,
            };
            conn.send(&read_request)?;

            match conn.receive()? {
                Response::Data { bytes } => Ok(bytes),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        });

        match result {
            Ok(bytes) => {
                self.offset.set(current_offset + bytes.len() as u64);
                if bytes.is_empty() {
                    Err(exports::wasi::io::streams::StreamError::Closed)
                } else {
                    Ok(bytes)
                }
            }
            Err(_) => Err(
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                }),
            ),
        }
    }

    fn skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        self.blocking_skip(len)
    }

    fn blocking_skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        let current_offset = self.offset.get();
        self.offset.set(current_offset + len);
        Ok(len)
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        // Return an always-ready pollable since RPC is blocking
        exports::wasi::io::poll::Pollable::new(AlwaysReadyPollable)
    }
}

// File output stream implementation for write_via_stream
struct FileOutputStream {
    handle: u32,       // Descriptor handle
    offset: Cell<u64>, // Current write position
    append: bool,      // Append mode - seek to end before each write
}

impl exports::wasi::io::streams::GuestOutputStream for FileOutputStream {
    fn check_write(&self) -> Result<u64, exports::wasi::io::streams::StreamError> {
        // Always ready to accept writes (up to 64KB at a time)
        Ok(65536)
    }

    fn write(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.blocking_write_and_flush(contents)
    }

    fn blocking_write_and_flush(
        &self,
        contents: Vec<u8>,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle)).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })?;

        let current_offset = self.offset.get();

        let result = if self.append {
            // Use atomic AppendWrite for append mode (no separate Seek needed)
            with_connection(|conn| {
                let request = Request::AppendWrite {
                    fd: server_fd,
                    data: contents,
                };
                conn.send(&request)?;

                match conn.receive()? {
                    Response::Written { count } => Ok(count as u64),
                    Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                    _ => Err(ErrorCode::Io),
                }
            })
        } else {
            // Non-append mode: Seek + Write
            with_connection(|conn| {
                let seek_request = Request::Seek {
                    fd: server_fd,
                    offset: current_offset as i64,
                    whence: 0, // SEEK_SET
                };
                conn.send(&seek_request)?;

                match conn.receive()? {
                    Response::Position { .. } => {}
                    Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                    _ => return Err(ErrorCode::Io),
                }

                let write_request = Request::Write {
                    fd: server_fd,
                    data: contents,
                };
                conn.send(&write_request)?;

                match conn.receive()? {
                    Response::Written { count } => Ok(count as u64),
                    Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                    _ => Err(ErrorCode::Io),
                }
            })
        };

        match result {
            Ok(written) => {
                // Don't update offset for append mode (always seek to end)
                if !self.append {
                    self.offset.set(current_offset + written);
                }
                Ok(())
            }
            Err(_) => Err(
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                }),
            ),
        }
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        Ok(())
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        Ok(())
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        // Return an always-ready pollable since RPC is blocking
        exports::wasi::io::poll::Pollable::new(AlwaysReadyPollable)
    }

    fn write_zeroes(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        let zeroes = vec![0u8; len as usize];
        self.blocking_write_and_flush(zeroes)
    }

    fn blocking_write_zeroes_and_flush(
        &self,
        len: u64,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.write_zeroes(len)
    }

    fn splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            }),
        )
    }

    fn blocking_splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            }),
        )
    }
}

// Dummy pollable that's always ready (avoids nested runtime issue with WASI imports)
struct AlwaysReadyPollable;

impl exports::wasi::io::poll::GuestPollable for AlwaysReadyPollable {
    fn ready(&self) -> bool {
        true // Always ready
    }

    fn block(&self) {
        // No-op: already ready
    }
}

// NOTE: Stream operations (write_via_stream, read_via_stream) are handled directly
// in vfs-rpc-host using Descriptor::write/read to avoid nested runtime issues
// (WASI exports cannot call WASI imports from within an export call).

impl exports::wasi::filesystem::types::GuestDirectoryEntryStream for DirectoryEntryStreamImpl {
    fn read_directory_entry(&self) -> Result<Option<DirectoryEntry>, ErrorCode> {
        let entries = self.entries.borrow();
        let index = self.index.get();

        if index >= entries.len() {
            Ok(None)
        } else {
            let entry = entries[index].clone();
            self.index.set(index + 1);
            Ok(Some(entry))
        }
    }
}

// Implement Guest trait for wasi:io/error
impl exports::wasi::io::error::Guest for RpcAdapter {
    type Error = PassthroughError;
}

// Implement Guest trait for wasi:io/streams
impl exports::wasi::io::streams::Guest for RpcAdapter {
    type InputStream = UnifiedInputStream;
    type OutputStream = UnifiedOutputStream;
}

// Implement Guest trait for wasi:io/poll
impl exports::wasi::io::poll::Guest for RpcAdapter {
    type Pollable = PassthroughPollable;

    fn poll(pollables: Vec<exports::wasi::io::poll::PollableBorrow<'_>>) -> Vec<u32> {
        // All pollables are ready (RPC operations block at I/O level)
        (0..pollables.len() as u32).collect()
    }
}

// Passthrough implementations for CLI interfaces
impl exports::wasi::cli::stdin::Guest for RpcAdapter {
    fn get_stdin() -> exports::wasi::cli::stdin::InputStream {
        let inner = wasi::cli::stdin::get_stdin();
        exports::wasi::io::streams::InputStream::new(UnifiedInputStream::Passthrough(inner))
    }
}

impl exports::wasi::cli::stdout::Guest for RpcAdapter {
    fn get_stdout() -> exports::wasi::cli::stdout::OutputStream {
        let inner = wasi::cli::stdout::get_stdout();
        exports::wasi::io::streams::OutputStream::new(UnifiedOutputStream::Passthrough(inner))
    }
}

impl exports::wasi::cli::stderr::Guest for RpcAdapter {
    fn get_stderr() -> exports::wasi::cli::stderr::OutputStream {
        let inner = wasi::cli::stderr::get_stderr();
        exports::wasi::io::streams::OutputStream::new(UnifiedOutputStream::Passthrough(inner))
    }
}

// Passthrough implementation for monotonic-clock
impl exports::wasi::clocks::monotonic_clock::Guest for RpcAdapter {
    fn now() -> exports::wasi::clocks::monotonic_clock::Instant {
        wasi::clocks::monotonic_clock::now()
    }

    fn resolution() -> exports::wasi::clocks::monotonic_clock::Duration {
        wasi::clocks::monotonic_clock::resolution()
    }

    fn subscribe_instant(
        when: exports::wasi::clocks::monotonic_clock::Instant,
    ) -> exports::wasi::clocks::monotonic_clock::Pollable {
        let inner = wasi::clocks::monotonic_clock::subscribe_instant(when);
        exports::wasi::io::poll::Pollable::new(PassthroughPollable { inner })
    }

    fn subscribe_duration(
        when: exports::wasi::clocks::monotonic_clock::Duration,
    ) -> exports::wasi::clocks::monotonic_clock::Pollable {
        let inner = wasi::clocks::monotonic_clock::subscribe_duration(when);
        exports::wasi::io::poll::Pollable::new(PassthroughPollable { inner })
    }
}

// Unified stream types that can be either file-based or passthrough
enum UnifiedInputStream {
    File(FileInputStream),
    Passthrough(wasi::io::streams::InputStream),
}

impl exports::wasi::io::streams::GuestInputStream for UnifiedInputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.read(len),
            UnifiedInputStream::Passthrough(p) => p.read(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.blocking_read(len),
            UnifiedInputStream::Passthrough(p) => p.blocking_read(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.skip(len),
            UnifiedInputStream::Passthrough(p) => p.skip(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.blocking_skip(len),
            UnifiedInputStream::Passthrough(p) => p.blocking_skip(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        match self {
            UnifiedInputStream::File(f) => f.subscribe(),
            UnifiedInputStream::Passthrough(p) => {
                exports::wasi::io::poll::Pollable::new(PassthroughPollable {
                    inner: p.subscribe(),
                })
            }
        }
    }
}

enum UnifiedOutputStream {
    File(FileOutputStream),
    Passthrough(wasi::io::streams::OutputStream),
}

impl exports::wasi::io::streams::GuestOutputStream for UnifiedOutputStream {
    fn check_write(&self) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.check_write(),
            UnifiedOutputStream::Passthrough(p) => p.check_write().map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn write(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.write(contents),
            UnifiedOutputStream::Passthrough(p) => p.write(&contents).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_write_and_flush(
        &self,
        contents: Vec<u8>,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.blocking_write_and_flush(contents),
            UnifiedOutputStream::Passthrough(p) => {
                p.blocking_write_and_flush(&contents).map_err(|_| {
                    exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                        exports::wasi::io::error::Error::from_handle(0)
                    })
                })
            }
        }
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.flush(),
            UnifiedOutputStream::Passthrough(p) => p.flush().map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.blocking_flush(),
            UnifiedOutputStream::Passthrough(p) => p.blocking_flush().map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        match self {
            UnifiedOutputStream::File(f) => f.subscribe(),
            UnifiedOutputStream::Passthrough(p) => {
                exports::wasi::io::poll::Pollable::new(PassthroughPollable {
                    inner: p.subscribe(),
                })
            }
        }
    }

    fn write_zeroes(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.write_zeroes(len),
            UnifiedOutputStream::Passthrough(p) => p.write_zeroes(len).map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            }),
        }
    }

    fn blocking_write_zeroes_and_flush(
        &self,
        len: u64,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.blocking_write_zeroes_and_flush(len),
            UnifiedOutputStream::Passthrough(p) => {
                p.blocking_write_zeroes_and_flush(len).map_err(|_| {
                    exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                        exports::wasi::io::error::Error::from_handle(0)
                    })
                })
            }
        }
    }

    fn splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            }),
        )
    }

    fn blocking_splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            }),
        )
    }
}

// Passthrough stream implementations (kept for reference, but no longer used directly)
struct PassthroughInputStream {
    inner: wasi::io::streams::InputStream,
}

impl exports::wasi::io::streams::GuestInputStream for PassthroughInputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        self.inner.read(len).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        self.inner.blocking_read(len).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        self.inner.skip(len).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn blocking_skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        self.inner.blocking_skip(len).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        exports::wasi::io::poll::Pollable::new(PassthroughPollable {
            inner: self.inner.subscribe(),
        })
    }
}

struct PassthroughOutputStream {
    inner: wasi::io::streams::OutputStream,
}

impl exports::wasi::io::streams::GuestOutputStream for PassthroughOutputStream {
    fn check_write(&self) -> Result<u64, exports::wasi::io::streams::StreamError> {
        self.inner.check_write().map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn write(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.inner.write(&contents).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn blocking_write_and_flush(
        &self,
        contents: Vec<u8>,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.inner.blocking_write_and_flush(&contents).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.inner.flush().map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.inner.blocking_flush().map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        exports::wasi::io::poll::Pollable::new(PassthroughPollable {
            inner: self.inner.subscribe(),
        })
    }

    fn write_zeroes(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.inner.write_zeroes(len).map_err(|_| {
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            })
        })
    }

    fn blocking_write_zeroes_and_flush(
        &self,
        len: u64,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        self.inner
            .blocking_write_zeroes_and_flush(len)
            .map_err(|_| {
                exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                    exports::wasi::io::error::Error::from_handle(0)
                })
            })
    }

    fn splice(
        &self,
        src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        // Not yet implemented
        Err(
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            }),
        )
    }

    fn blocking_splice(
        &self,
        src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        // Not yet implemented
        Err(
            exports::wasi::io::streams::StreamError::LastOperationFailed(unsafe {
                exports::wasi::io::error::Error::from_handle(0)
            }),
        )
    }
}

struct PassthroughPollable {
    inner: wasi::io::poll::Pollable,
}

impl exports::wasi::io::poll::GuestPollable for PassthroughPollable {
    fn ready(&self) -> bool {
        self.inner.ready()
    }

    fn block(&self) {
        self.inner.block()
    }
}

// Passthrough Error implementation
struct PassthroughError {
    inner: Option<wasi::io::error::Error>,
}

impl exports::wasi::io::error::GuestError for PassthroughError {
    fn to_debug_string(&self) -> String {
        self.inner
            .as_ref()
            .map(|e| e.to_debug_string())
            .unwrap_or_else(|| "Unknown error".to_string())
    }
}
