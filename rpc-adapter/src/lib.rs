// RPC Adapter - WASI filesystem adapter that makes RPC calls to vfs-rpc-server
//
// This is a component that exports WASI filesystem interfaces
// and delegates to vfs-rpc-server via TCP RPC calls.

#![no_main]
#![allow(warnings)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Once;

use vfs_rpc_protocol::{Request, Response, PROTOCOL_VERSION};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "rpc-adapter",
    path: "../wit",
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
use wasi::sockets::tcp_create_socket::create_tcp_socket;

// RPC connection using TCP sockets
struct RpcConnection {
    input_stream: InputStream,
    output_stream: OutputStream,
    session_id: u64,
}

impl RpcConnection {
    fn connect() -> Result<Self, ErrorCode> {
        // Create TCP socket
        let network = instance_network();
        let socket = create_tcp_socket(IpAddressFamily::Ipv4).map_err(|_| ErrorCode::Io)?;

        // Connect to localhost:9000
        let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port: 9000,
            address: (127, 0, 0, 1),
        });

        socket
            .start_connect(&network, addr)
            .map_err(|_| ErrorCode::Io)?;

        // Wait for connection to complete
        let (input_stream, output_stream) = loop {
            match socket.finish_connect() {
                Ok(streams) => break streams,
                Err(_) => {
                    let pollable = socket.subscribe();
                    poll(&[&pollable]);
                }
            }
        };

        // Do handshake
        let mut conn = RpcConnection {
            input_stream,
            output_stream,
            session_id: 0,
        };

        conn.send_request(&Request::Connect {
            version: PROTOCOL_VERSION,
        })?;
        match conn.receive_response()? {
            Response::Connected { session_id, .. } => {
                conn.session_id = session_id;
                Ok(conn)
            }
            _ => Err(ErrorCode::Io),
        }
    }

    fn send_request(&mut self, request: &Request) -> Result<(), ErrorCode> {
        let data = serde_json::to_vec(request).map_err(|_| ErrorCode::Io)?;
        let len = (data.len() as u32).to_be_bytes();

        // Write length prefix
        loop {
            match self.output_stream.blocking_write_and_flush(&len) {
                Ok(()) => break,
                Err(_) => {
                    let pollable = self.output_stream.subscribe();
                    poll(&[&pollable]);
                }
            }
        }

        // Write JSON payload
        loop {
            match self.output_stream.blocking_write_and_flush(&data) {
                Ok(()) => break,
                Err(_) => {
                    let pollable = self.output_stream.subscribe();
                    poll(&[&pollable]);
                }
            }
        }

        Ok(())
    }

    fn receive_response(&mut self) -> Result<Response, ErrorCode> {
        // Read 4-byte length prefix with retry on would-block and partial reads
        let mut len_buf = Vec::new();
        while len_buf.len() < 4 {
            match self.input_stream.blocking_read(4 - len_buf.len() as u64) {
                Ok(bytes) if !bytes.is_empty() => {
                    len_buf.extend_from_slice(&bytes);
                }
                _ => {
                    let pollable = self.input_stream.subscribe();
                    poll(&[&pollable]);
                }
            }
        }

        let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;

        // Read message body with retry on would-block and partial reads
        let mut data = Vec::new();
        while (data.len() as u64) < len {
            match self.input_stream.blocking_read(len - data.len() as u64) {
                Ok(bytes) if !bytes.is_empty() => {
                    data.extend_from_slice(&bytes);
                }
                _ => {
                    let pollable = self.input_stream.subscribe();
                    poll(&[&pollable]);
                }
            }
        }

        serde_json::from_slice(&data).map_err(|_| ErrorCode::Io)
    }
}

// Main RPC adapter state
static INIT: Once = Once::new();
static mut RPC_STATE: Option<RpcState> = None;

struct RpcState {
    connection: RefCell<RpcConnection>,
    // Map descriptor handle to server FD
    descriptor_to_fd: RefCell<BTreeMap<u32, u32>>,
    // Map server FD to descriptor handle
    fd_to_descriptor: RefCell<BTreeMap<u32, u32>>,
    next_descriptor: RefCell<u32>,
}

impl RpcState {
    fn new() -> Result<Self, ErrorCode> {
        let connection = RpcConnection::connect()?;

        let state = Self {
            connection: RefCell::new(connection),
            descriptor_to_fd: RefCell::new(BTreeMap::new()),
            fd_to_descriptor: RefCell::new(BTreeMap::new()),
            next_descriptor: RefCell::new(1),
        };

        // Register root directory as descriptor 0, server FD 0
        state.descriptor_to_fd.borrow_mut().insert(0, 0);
        state.fd_to_descriptor.borrow_mut().insert(0, 0);

        Ok(state)
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
fn with_rpc_state<F, R>(f: F) -> Result<R, ErrorCode>
where
    F: FnOnce(&RpcState) -> R,
{
    unsafe {
        INIT.call_once(|| {
            if let Ok(state) = RpcState::new() {
                RPC_STATE = Some(state);
            }
        });
        RPC_STATE.as_ref().map(f).ok_or(ErrorCode::Io)
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

        // Initialize state and verify connection
        // This ensures RPC connection is established before returning descriptor
        match with_rpc_state(|state| {
            // Verify that descriptor 0 is mapped
            state.descriptor_to_fd.borrow().get(&0).copied()
        }) {
            Ok(Some(fd)) => {
                eprintln!("[RPC-ADAPTER] State verified: descriptor 0 -> fd {}", fd);
                // Connection established and descriptor 0 is mapped
                let desc = Descriptor::new(DescriptorImpl { handle: 0 });
                eprintln!("[RPC-ADAPTER] Returning descriptor 0 for path /");
                vec![(desc, "/".to_string())]
            }
            Ok(None) => {
                eprintln!("[RPC-ADAPTER] ERROR: descriptor 0 not mapped");
                // State exists but descriptor 0 not mapped - this shouldn't happen
                // but return empty to signal error
                vec![]
            }
            Err(e) => {
                eprintln!("[RPC-ADAPTER] ERROR: Failed to initialize state: {:?}", e);
                // Failed to initialize RPC state (connection failed)
                // Return empty vector to signal no preopens available
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
        _offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::InputStream, ErrorCode> {
        // Not yet implemented - would create a stream wrapper
        Err(ErrorCode::Unsupported)
    }

    fn write_via_stream(
        &self,
        _offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        // Not yet implemented - would create a stream wrapper
        Err(ErrorCode::Unsupported)
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        // Not yet implemented - would create a stream wrapper
        Err(ErrorCode::Unsupported)
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
        with_rpc_state(|state| {
            if self.handle == 0 {
                return Ok(DescriptorType::Directory);
            }

            let server_fd = state.get_server_fd(self.handle)?;

            let request = Request::Fstat { fd: server_fd };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
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
        })?
    }

    fn set_size(&self, size: Filesize) -> Result<(), ErrorCode> {
        with_rpc_state(|state| {
            let server_fd = state.get_server_fd(self.handle)?;

            let request = Request::Ftruncate {
                fd: server_fd,
                size,
            };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Ok => Ok(()),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
    }

    fn set_times(
        &self,
        _data_access_timestamp: NewTimestamp,
        _data_modification_timestamp: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn read(&self, length: Filesize, offset: Filesize) -> Result<(Vec<u8>, bool), ErrorCode> {
        with_rpc_state(|state| {
            let server_fd = state.get_server_fd(self.handle)?;

            // Seek to offset
            let request = Request::Seek {
                fd: server_fd,
                offset: offset as i64,
                whence: 0,
            };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Position { .. } => {}
                Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                _ => return Err(ErrorCode::Io),
            }

            // Read data
            let request = Request::Read {
                fd: server_fd,
                length: length as usize,
            };
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Data { bytes } => {
                    let end_of_stream = bytes.len() < length as usize;
                    Ok((bytes, end_of_stream))
                }
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
    }

    fn write(&self, buffer: Vec<u8>, offset: Filesize) -> Result<Filesize, ErrorCode> {
        with_rpc_state(|state| {
            let server_fd = state.get_server_fd(self.handle)?;

            // Seek to offset
            let request = Request::Seek {
                fd: server_fd,
                offset: offset as i64,
                whence: 0,
            };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Position { .. } => {}
                Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                _ => return Err(ErrorCode::Io),
            }

            // Write data
            let len = buffer.len();
            let request = Request::Write {
                fd: server_fd,
                data: buffer,
            };
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Written { count } => Ok(count as Filesize),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
    }

    fn read_directory(
        &self,
    ) -> Result<exports::wasi::filesystem::types::DirectoryEntryStream, ErrorCode> {
        with_rpc_state(|state| {
            let server_fd = state.get_server_fd(self.handle)?;

            let request = Request::ReaddirFd { fd: server_fd };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
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
        })?
    }

    fn sync(&self) -> Result<(), ErrorCode> {
        Ok(())
    }

    fn create_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        with_rpc_state(|state| {
            // For root directory, use direct path
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                // Would need to track paths, for now just use relative
                path
            };

            let request = Request::Mkdir { path: full_path };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Ok => Ok(()),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
    }

    fn stat(&self) -> Result<DescriptorStat, ErrorCode> {
        with_rpc_state(|state| {
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

            let server_fd = state.get_server_fd(self.handle)?;

            let request = Request::Fstat { fd: server_fd };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
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
        })?
    }

    fn stat_at(&self, _path_flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        with_rpc_state(|state| {
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                path
            };

            let request = Request::Stat { path: full_path };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
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
        })?
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

        with_rpc_state(|state| {
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
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Fd { fd: server_fd } => {
                    let desc_id = state.allocate_descriptor(server_fd);
                    Ok(Descriptor::new(DescriptorImpl { handle: desc_id }))
                }
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
    }

    fn readlink_at(&self, _path: String) -> Result<String, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn remove_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        with_rpc_state(|state| {
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                path
            };

            let request = Request::Rmdir { path: full_path };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Ok => Ok(()),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
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
        with_rpc_state(|state| {
            let full_path = if self.handle == 0 {
                format!("/{}", path.trim_start_matches('/'))
            } else {
                path
            };

            let request = Request::Unlink { path: full_path };
            let mut conn = state.connection.borrow_mut();
            conn.send_request(&request)?;

            match conn.receive_response()? {
                Response::Ok => Ok(()),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })?
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
    type InputStream = PassthroughInputStream;
    type OutputStream = PassthroughOutputStream;
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
        exports::wasi::io::streams::InputStream::new(PassthroughInputStream { inner })
    }
}

impl exports::wasi::cli::stdout::Guest for RpcAdapter {
    fn get_stdout() -> exports::wasi::cli::stdout::OutputStream {
        let inner = wasi::cli::stdout::get_stdout();
        exports::wasi::io::streams::OutputStream::new(PassthroughOutputStream { inner })
    }
}

impl exports::wasi::cli::stderr::Guest for RpcAdapter {
    fn get_stderr() -> exports::wasi::cli::stderr::OutputStream {
        let inner = wasi::cli::stderr::get_stderr();
        exports::wasi::io::streams::OutputStream::new(PassthroughOutputStream { inner })
    }
}

// Passthrough stream implementations
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
