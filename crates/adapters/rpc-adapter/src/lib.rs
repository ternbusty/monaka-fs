// RPC Adapter: WASI filesystem adapter that makes RPC calls to vfs-rpc-server
//
// This is a component that exports WASI filesystem interfaces
// and delegates to vfs-rpc-server via TCP RPC calls.
//
// Design: Uses persistent TCP connection with WASI poll for efficient I/O.
// Socket is kept in PersistentConnection to prevent premature drop.
// subscribe() creates child Pollables, but they are dropped within each loop iteration.

#![cfg_attr(not(test), no_main)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use vfs_rpc_protocol::{
    from_proto_response_bytes, to_proto_request_bytes, ErrorCode as RpcErrorCode, Request,
    Response, RpcRequestMessage, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION,
};

// WIT bindgen generates the bindings
wit_bindgen::generate!({
    world: "rpc-adapter",
    path: "../../../wit",
    generate_all,
});

// Re-export for convenience
use exports::wasi::filesystem::types::{
    Descriptor, DescriptorBorrow, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    ErrorCode, Filesize, NewTimestamp, OpenFlags, PathFlags,
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

thread_local! {
    static RPC_CONNECTION: RefCell<Option<PersistentConnection>> = const { RefCell::new(None) };
}

struct PersistentConnection {
    // Socket is kept alive to prevent premature drop (streams are children of socket)
    #[allow(dead_code)]
    socket: TcpSocket,
    input_stream: InputStream,
    output_stream: OutputStream,
    session_id: String,
    /// Protocol version negotiated with the server. Requests that only
    /// exist in newer versions (`Fsync`) are skipped against old servers.
    version: u32,
}

/// Server port: `VFS_RPC_PORT`, falling back to the protocol default.
fn rpc_port() -> u16 {
    std::env::var("VFS_RPC_PORT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(vfs_rpc_protocol::DEFAULT_PORT)
}

/// Total time to keep retrying an open that the server answers with
/// `Busy` (another instance holds the file's S3 lease). Mirrors the
/// server-side `VFS_S3_FILE_LOCK_TIMEOUT_MS` default.
const DEFAULT_LOCK_TIMEOUT_MS: u64 = 10_000;

fn lock_timeout_budget_ms() -> u64 {
    std::env::var("VFS_S3_FILE_LOCK_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_LOCK_TIMEOUT_MS)
}

/// Delays (ms) between successive `Busy` retries: doubling from 20 ms,
/// capped at 500 ms, summing to at most `budget_ms`.
fn busy_backoff_schedule(budget_ms: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut spent = 0;
    let mut step = 20;
    while spent < budget_ms {
        let d = step.min(budget_ms - spent);
        out.push(d);
        spent += d;
        step = (step * 2).min(500);
    }
    out
}

fn sleep_ms(ms: u64) {
    let pollable = wasi::clocks::monotonic_clock::subscribe_duration(ms * 1_000_000);
    poll(&[&pollable]);
}

impl PersistentConnection {
    fn connect() -> Result<Self, ErrorCode> {
        // Create TCP socket
        let network = instance_network();
        let socket = create_tcp_socket(IpAddressFamily::Ipv4).map_err(|_| ErrorCode::Io)?;

        // Connect to localhost:<VFS_RPC_PORT>
        let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port: rpc_port(),
            address: (127, 0, 0, 1),
        });

        socket
            .start_connect(&network, addr)
            .map_err(|_| ErrorCode::Io)?;

        // Wait for connection to complete using poll() for efficient waiting
        let (mut input_stream, mut output_stream) = loop {
            match socket.finish_connect() {
                Ok(streams) => break streams,
                Err(wasi::sockets::network::ErrorCode::WouldBlock) => {
                    let pollable = socket.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                Err(_) => return Err(ErrorCode::Io),
            }
        };

        // Increase TCP buffer sizes for better throughput on large transfers
        const TCP_BUF_SIZE: u64 = 4 * 1024 * 1024; // 4MB
        let _ = socket.set_receive_buffer_size(TCP_BUF_SIZE);
        let _ = socket.set_send_buffer_size(TCP_BUF_SIZE);

        // Handshake. A server older than this adapter rejects the current
        // version with ProtocolError; fall back to the oldest version we
        // still speak so the two keep working together.
        for version in [PROTOCOL_VERSION, MIN_PROTOCOL_VERSION] {
            Self::send_raw(&mut output_stream, None, &Request::Connect { version })?;
            match Self::receive_raw(&mut input_stream) {
                Ok(Response::Connected {
                    session_id,
                    version: negotiated,
                }) => {
                    return Ok(Self {
                        socket,
                        input_stream,
                        output_stream,
                        session_id,
                        version: negotiated,
                    })
                }
                Ok(Response::Error {
                    code: RpcErrorCode::ProtocolError,
                    ..
                }) if version != MIN_PROTOCOL_VERSION => continue,
                Ok(_) => return Err(ErrorCode::Io),
                Err(e) => return Err(e),
            }
        }
        Err(ErrorCode::Io)
    }

    fn send_raw(
        output_stream: &mut OutputStream,
        session_id: Option<String>,
        request: &Request,
    ) -> Result<(), ErrorCode> {
        let rpc_request = RpcRequestMessage {
            session_id,
            request: request.clone(),
        };
        let data = to_proto_request_bytes(&rpc_request);
        let len = (data.len() as u32).to_be_bytes();

        // Write length prefix + data together
        let mut payload = Vec::with_capacity(4 + data.len());
        payload.extend_from_slice(&len);
        payload.extend_from_slice(&data);

        // Use non-blocking write with check_write to get larger buffer sizes
        let mut offset = 0;
        while offset < payload.len() {
            let available = output_stream.check_write().map_err(|_| ErrorCode::Io)? as usize;
            if available == 0 {
                let pollable = output_stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
            let end = std::cmp::min(offset + available, payload.len());
            output_stream
                .write(&payload[offset..end])
                .map_err(|_| ErrorCode::Io)?;
            offset = end;
        }
        output_stream.blocking_flush().map_err(|_| ErrorCode::Io)?;
        Ok(())
    }

    fn receive_raw(input_stream: &mut InputStream) -> Result<Response, ErrorCode> {
        // Read 4-byte length prefix
        let mut len_buf = Vec::new();

        while len_buf.len() < 4 {
            let remaining = 4 - len_buf.len() as u64;
            let bytes = match input_stream.blocking_read(remaining) {
                Ok(b) => b,
                Err(_) => return Err(ErrorCode::Io),
            };
            if bytes.is_empty() {
                let pollable = input_stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
            len_buf.extend_from_slice(&bytes);
        }

        let len = u32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]) as u64;

        // Read message body - pre-allocate buffer to avoid reallocations
        let mut data = Vec::with_capacity(len as usize);

        while (data.len() as u64) < len {
            let remaining = len - data.len() as u64;
            let bytes = match input_stream.blocking_read(remaining) {
                Ok(b) => b,
                Err(_) => return Err(ErrorCode::Io),
            };
            if bytes.is_empty() {
                let pollable = input_stream.subscribe();
                poll(&[&pollable]);
                continue;
            }
            data.extend_from_slice(&bytes);
        }

        from_proto_response_bytes(&data).map_err(rpc_error_to_wasi)
    }

    fn send(&mut self, request: &Request) -> Result<(), ErrorCode> {
        Self::send_raw(
            &mut self.output_stream,
            Some(self.session_id.clone()),
            request,
        )
    }

    fn receive(&mut self) -> Result<Response, ErrorCode> {
        Self::receive_raw(&mut self.input_stream)
    }

    fn call(&mut self, request: &Request) -> Result<Response, ErrorCode> {
        self.send(request)?;
        self.receive()
    }
}

// Get or initialize the persistent connection
fn with_connection<F, R>(f: F) -> Result<R, ErrorCode>
where
    F: FnOnce(&mut PersistentConnection) -> Result<R, ErrorCode>,
{
    RPC_CONNECTION.with(|cell| {
        if cell.borrow().is_none() {
            if let Ok(conn) = PersistentConnection::connect() {
                *cell.borrow_mut() = Some(conn);
            }
        }
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(conn) => f(conn),
            None => Err(ErrorCode::Io),
        }
    })
}

// Helper to make RPC call using persistent connection
fn rpc_call(request: &Request) -> Result<Response, ErrorCode> {
    with_connection(|conn| conn.call(request))
}

// Main RPC adapter state: only stores descriptor mappings, no connection
thread_local! {
    static RPC_STATE: RefCell<Option<RpcState>> = const { RefCell::new(None) };
}

struct RpcState {
    // Map descriptor handle to server FD
    descriptor_to_fd: RefCell<BTreeMap<u32, u32>>,
    // Map server FD to descriptor handle
    fd_to_descriptor: RefCell<BTreeMap<u32, u32>>,
    // fs-core open flags per descriptor handle (write access decides
    // whether close / fsync talk to the S3 lease on the server).
    descriptor_flags: RefCell<BTreeMap<u32, u32>>,
    // Output streams still alive per descriptor handle. Their buffers must
    // reach the server before `Close`, so a descriptor dropped while a
    // stream is alive defers its `Close` to the last stream drop.
    live_streams: RefCell<BTreeMap<u32, Vec<Weak<FileOutputStream>>>>,
    close_pending: RefCell<BTreeSet<u32>>,
    next_descriptor: RefCell<u32>,
}

impl RpcState {
    fn new() -> Self {
        let state = Self {
            descriptor_to_fd: RefCell::new(BTreeMap::new()),
            fd_to_descriptor: RefCell::new(BTreeMap::new()),
            descriptor_flags: RefCell::new(BTreeMap::new()),
            live_streams: RefCell::new(BTreeMap::new()),
            close_pending: RefCell::new(BTreeSet::new()),
            next_descriptor: RefCell::new(1),
        };

        // Register root directory as descriptor 0, server FD 0
        state.descriptor_to_fd.borrow_mut().insert(0, 0);
        state.fd_to_descriptor.borrow_mut().insert(0, 0);

        state
    }

    fn allocate_descriptor(&self, server_fd: u32, flags: u32) -> u32 {
        let desc = *self.next_descriptor.borrow();
        *self.next_descriptor.borrow_mut() += 1;
        self.descriptor_to_fd.borrow_mut().insert(desc, server_fd);
        self.fd_to_descriptor.borrow_mut().insert(server_fd, desc);
        self.descriptor_flags.borrow_mut().insert(desc, flags);
        desc
    }

    /// Forget a descriptor and return its server fd for the `Close` call.
    fn release(&self, descriptor: u32) -> Option<u32> {
        let fd = self.descriptor_to_fd.borrow_mut().remove(&descriptor)?;
        self.fd_to_descriptor.borrow_mut().remove(&fd);
        self.descriptor_flags.borrow_mut().remove(&descriptor);
        self.live_streams.borrow_mut().remove(&descriptor);
        self.close_pending.borrow_mut().remove(&descriptor);
        Some(fd)
    }

    fn stream_opened(&self, descriptor: u32, stream: Weak<FileOutputStream>) {
        self.live_streams
            .borrow_mut()
            .entry(descriptor)
            .or_default()
            .push(stream);
    }

    fn live_stream_count(&self, descriptor: u32) -> usize {
        self.live_streams
            .borrow()
            .get(&descriptor)
            .map(|v| v.iter().filter(|w| w.strong_count() > 0).count())
            .unwrap_or(0)
    }

    fn live_streams_for(&self, descriptor: u32) -> Vec<Rc<FileOutputStream>> {
        self.live_streams
            .borrow()
            .get(&descriptor)
            .map(|v| v.iter().filter_map(|w| w.upgrade()).collect())
            .unwrap_or_default()
    }

    /// A stream for `descriptor` finished flushing and is gone. Returns
    /// the server fd to `Close` when the descriptor was already dropped
    /// and this was its last stream.
    fn stream_closed(&self, descriptor: u32) -> Option<u32> {
        if let Some(v) = self.live_streams.borrow_mut().get_mut(&descriptor) {
            v.retain(|w| w.strong_count() > 0);
        }
        if self.live_stream_count(descriptor) == 0
            && self.close_pending.borrow().contains(&descriptor)
        {
            return self.release(descriptor);
        }
        None
    }

    /// The descriptor resource was dropped. Returns the server fd to
    /// `Close` now, or `None` when live streams still have to flush first.
    fn descriptor_dropped(&self, descriptor: u32) -> Option<u32> {
        if self.live_stream_count(descriptor) > 0 {
            self.close_pending.borrow_mut().insert(descriptor);
            return None;
        }
        self.release(descriptor)
    }

    fn get_server_fd(&self, descriptor: u32) -> Result<u32, ErrorCode> {
        self.descriptor_to_fd
            .borrow()
            .get(&descriptor)
            .copied()
            .ok_or(ErrorCode::BadDescriptor)
    }
}

// Helper to get or initialize RPC state
fn with_rpc_state<F, R>(f: F) -> R
where
    F: FnOnce(&RpcState) -> R,
{
    RPC_STATE.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(RpcState::new());
        }
        let borrow = cell.borrow();
        f(borrow.as_ref().expect("RPC state initialized above"))
    })
}

// Convert RPC error to WASI error code
fn rpc_error_to_wasi(code: RpcErrorCode) -> ErrorCode {
    match code {
        RpcErrorCode::NotFound => ErrorCode::NoEntry,
        RpcErrorCode::NotADirectory => ErrorCode::NotDirectory,
        RpcErrorCode::IsADirectory => ErrorCode::IsDirectory,
        RpcErrorCode::InvalidArgument => ErrorCode::Invalid,
        RpcErrorCode::BadFileDescriptor => ErrorCode::BadDescriptor,
        RpcErrorCode::PermissionDenied => ErrorCode::Access,
        RpcErrorCode::AlreadyExists => ErrorCode::Exist,
        RpcErrorCode::NotEmpty => ErrorCode::NotEmpty,
        RpcErrorCode::Busy => ErrorCode::Busy,
        RpcErrorCode::Conflict => ErrorCode::NotRecoverable,
        _ => ErrorCode::Io,
    }
}

/// Send `Close` for a server fd from a drop path. Uses `try_with` because
/// drops can run during thread-local teardown at exit, when the connection
/// cell is already gone; a lost `Close` then only delays the server-side
/// lease release until the session disconnects.
fn send_close(server_fd: u32) {
    let _ = RPC_CONNECTION.try_with(|cell| {
        if let Some(conn) = cell.borrow_mut().as_mut() {
            let _ = conn.call(&Request::Close { fd: server_fd });
        }
    });
}

// Normalise a relative path coming from a WASI caller into the absolute form
// (`/foo/bar`) the RPC server expects.
fn normalize_path(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

// Build a `DescriptorStat` from RPC metadata. Timestamps are not carried by
// the protocol today, so they are returned as `None`.
fn make_descriptor_stat(is_dir: bool, size: u64) -> DescriptorStat {
    DescriptorStat {
        type_: if is_dir {
            DescriptorType::Directory
        } else {
            DescriptorType::RegularFile
        },
        link_count: 1,
        size: size as Filesize,
        data_access_timestamp: None,
        data_modification_timestamp: None,
        status_change_timestamp: None,
    }
}

// Issue a `Seek { fd, offset, whence: 0 }` request on the given persistent
// connection and consume the response. Used by the read/write paths that
// need to set the file cursor before doing IO atomically.
fn seek_via_conn(conn: &mut PersistentConnection, fd: u32, offset: u64) -> Result<(), ErrorCode> {
    let seek_request = Request::Seek {
        fd,
        offset: offset as i64,
        whence: 0,
    };
    conn.send(&seek_request)?;
    match conn.receive()? {
        Response::Position { .. } => Ok(()),
        Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
        _ => Err(ErrorCode::Io),
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
        let fd = with_rpc_state(|state| state.descriptor_to_fd.borrow().get(&0).copied());
        match fd {
            Some(_) => {
                let desc = Descriptor::new(DescriptorImpl {
                    handle: 0,
                    flags: 0,
                });
                vec![(desc, "/".to_string())]
            }
            None => vec![],
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
    /// fs-core open flags this descriptor was opened with.
    flags: u32,
}

impl DescriptorImpl {
    fn is_write(&self) -> bool {
        self.flags & 0x3 != 0
    }

    /// Push buffered writes to the server and ask it to flush the file to
    /// S3 (protocol v2). This is the only place a lease conflict can be
    /// reported to the application, because descriptor drop cannot return
    /// an error.
    fn fsync(&self) -> Result<(), ErrorCode> {
        if self.handle == 0 || !self.is_write() {
            return Ok(());
        }
        for stream in with_rpc_state(|state| state.live_streams_for(self.handle)) {
            stream.flush_buffer()?;
        }
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;
        with_connection(|conn| {
            if conn.version < 2 {
                return Ok(());
            }
            match conn.call(&Request::Fsync { fd: server_fd })? {
                Response::Ok => Ok(()),
                Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                _ => Err(ErrorCode::Io),
            }
        })
    }
}

impl Drop for DescriptorImpl {
    fn drop(&mut self) {
        if self.handle == 0 {
            return;
        }
        let fd = with_rpc_state(|state| state.descriptor_dropped(self.handle));
        if let Some(fd) = fd {
            send_close(fd);
        }
    }
}

impl exports::wasi::filesystem::types::GuestDescriptor for DescriptorImpl {
    fn read_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::InputStream, ErrorCode> {
        // Verify the descriptor is valid
        with_rpc_state(|state| state.get_server_fd(self.handle))?;

        Ok(exports::wasi::filesystem::types::InputStream::new(
            UnifiedInputStream::File(FileInputStream {
                handle: self.handle,
                offset: Cell::new(offset),
                buf: RefCell::new(Vec::new()),
                buf_offset: Cell::new(0),
            }),
        ))
    }

    fn write_via_stream(
        &self,
        offset: Filesize,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        // Verify the descriptor is valid
        with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let stream = Rc::new(FileOutputStream::new(self.handle, offset, false));
        with_rpc_state(|state| state.stream_opened(self.handle, Rc::downgrade(&stream)));
        Ok(exports::wasi::filesystem::types::OutputStream::new(
            UnifiedOutputStream::File(stream),
        ))
    }

    fn append_via_stream(
        &self,
    ) -> Result<exports::wasi::filesystem::types::OutputStream, ErrorCode> {
        // Verify the descriptor is valid
        with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let stream = Rc::new(FileOutputStream::new(self.handle, 0, true));
        with_rpc_state(|state| state.stream_opened(self.handle, Rc::downgrade(&stream)));
        Ok(exports::wasi::filesystem::types::OutputStream::new(
            UnifiedOutputStream::File(stream),
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
        self.fsync()
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
            seek_via_conn(conn, server_fd, offset)?;

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
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        // Use persistent connection to perform seek + write atomically
        with_connection(|conn| {
            seek_via_conn(conn, server_fd, offset)?;

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
        self.fsync()
    }

    fn create_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        // For root directory, use direct path
        let full_path = if self.handle == 0 {
            normalize_path(&path)
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
            return Ok(make_descriptor_stat(true, 0));
        }

        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;

        let request = Request::Fstat { fd: server_fd };
        match rpc_call(&request)? {
            Response::Metadata { metadata } => {
                Ok(make_descriptor_stat(metadata.is_dir, metadata.size))
            }
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn stat_at(&self, _path_flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        let full_path = if self.handle == 0 {
            normalize_path(&path)
        } else {
            path
        };

        let request = Request::Stat { path: full_path };
        match rpc_call(&request)? {
            Response::Metadata { metadata } => {
                Ok(make_descriptor_stat(metadata.is_dir, metadata.size))
            }
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
        let full_path = if self.handle == 0 {
            normalize_path(&path)
        } else {
            path
        };
        let fs_flags = convert_flags(open_flags, flags);

        let request = Request::OpenPath {
            path: full_path,
            flags: fs_flags,
        };

        // The server never waits for an S3 file lease (it would stall its
        // single-threaded loop); it answers Busy and this side retries
        // with backoff for up to the lock timeout budget.
        let mut delays = busy_backoff_schedule(lock_timeout_budget_ms()).into_iter();
        let server_fd = loop {
            match rpc_call(&request)? {
                Response::Fd { fd } => break fd,
                Response::Error {
                    code: RpcErrorCode::Busy,
                    ..
                } => match delays.next() {
                    Some(ms) => sleep_ms(ms),
                    None => return Err(ErrorCode::Busy),
                },
                Response::Error { code, .. } => return Err(rpc_error_to_wasi(code)),
                _ => return Err(ErrorCode::Io),
            }
        };

        let desc_id = with_rpc_state(|state| state.allocate_descriptor(server_fd, fs_flags));
        Ok(Descriptor::new(DescriptorImpl {
            handle: desc_id,
            flags: fs_flags,
        }))
    }

    fn readlink_at(&self, _path: String) -> Result<String, ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn remove_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        let full_path = if self.handle == 0 {
            normalize_path(&path)
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
        old_path: String,
        _new_descriptor: DescriptorBorrow<'_>,
        new_path: String,
    ) -> Result<(), ErrorCode> {
        let old_full = if self.handle == 0 {
            normalize_path(&old_path)
        } else {
            old_path
        };
        let new_full = if self.handle == 0 {
            normalize_path(&new_path)
        } else {
            new_path
        };
        let request = Request::Rename {
            old_path: old_full,
            new_path: new_full,
        };
        match rpc_call(&request)? {
            Response::Ok => Ok(()),
            Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
            _ => Err(ErrorCode::Io),
        }
    }

    fn symlink_at(&self, _old_path: String, _new_path: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Unsupported)
    }

    fn unlink_file_at(&self, path: String) -> Result<(), ErrorCode> {
        let full_path = if self.handle == 0 {
            normalize_path(&path)
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

const READ_AHEAD_SIZE: usize = 256 * 1024;

struct FileInputStream {
    handle: u32,
    offset: Cell<u64>,
    buf: RefCell<Vec<u8>>,
    buf_offset: Cell<u64>,
}

impl exports::wasi::io::streams::GuestInputStream for FileInputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        self.blocking_read(len)
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        let current_offset = self.offset.get();
        let len = len as usize;

        let buf_start = self.buf_offset.get();
        let buf = self.buf.borrow();
        let buf_end = buf_start + buf.len() as u64;

        if current_offset >= buf_start && current_offset + len as u64 <= buf_end {
            let local = (current_offset - buf_start) as usize;
            let data = buf[local..local + len].to_vec();
            drop(buf);
            self.offset.set(current_offset + data.len() as u64);
            return Ok(data);
        }
        drop(buf);

        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))
            .map_err(|_| exports::wasi::io::streams::StreamError::Closed)?;

        // Only read ahead when this request is a contiguous continuation of the
        // cached buffer. A fresh stream (empty buf) or a non-contiguous offset
        // (random access via a re-created stream) falls back to fetching just
        // the requested length, so random reads don't pay for wasted prefetch.
        let is_sequential = {
            let buf = self.buf.borrow();
            !buf.is_empty() && current_offset == buf_start + buf.len() as u64
        };
        let fetch_len = if is_sequential {
            len.max(READ_AHEAD_SIZE)
        } else {
            len
        };

        let result = with_connection(|conn| {
            seek_via_conn(conn, server_fd, current_offset)?;

            let read_request = Request::Read {
                fd: server_fd,
                length: fetch_len,
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
                if bytes.is_empty() {
                    return Err(exports::wasi::io::streams::StreamError::Closed);
                }
                let ret_len = bytes.len().min(len);
                let data = bytes[..ret_len].to_vec();
                self.buf_offset.set(current_offset);
                *self.buf.borrow_mut() = bytes;
                self.offset.set(current_offset + ret_len as u64);
                Ok(data)
            }
            Err(_) => Err(exports::wasi::io::streams::StreamError::Closed),
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
        exports::wasi::io::poll::Pollable::new(UnifiedPollable::AlwaysReady)
    }
}

// File output stream implementation for write_via_stream
struct FileOutputStream {
    handle: u32,              // Descriptor handle
    offset: Cell<u64>,        // Current write position
    append: bool,             // Append mode - seek to end before each write
    buffer: RefCell<Vec<u8>>, // Write buffer - flushed on drop
}

impl FileOutputStream {
    fn new(handle: u32, offset: u64, append: bool) -> Self {
        Self {
            handle,
            offset: Cell::new(offset),
            append,
            buffer: RefCell::new(Vec::new()),
        }
    }

    fn flush_buffer(&self) -> Result<(), ErrorCode> {
        let data: Vec<u8> = self.buffer.borrow_mut().drain(..).collect();
        if data.is_empty() {
            return Ok(());
        }

        let data_len = data.len();
        let server_fd = with_rpc_state(|state| state.get_server_fd(self.handle))?;
        let start_offset = self.offset.get();

        let result = if self.append {
            with_connection(|conn| {
                let request = Request::AppendWrite {
                    fd: server_fd,
                    data,
                };
                conn.send(&request)?;
                match conn.receive()? {
                    Response::Written { .. } => Ok(()),
                    Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                    _ => Err(ErrorCode::Io),
                }
            })
        } else {
            with_connection(|conn| {
                seek_via_conn(conn, server_fd, start_offset)?;

                // Write all data at once
                let write_request = Request::Write {
                    fd: server_fd,
                    data,
                };
                conn.send(&write_request)?;
                match conn.receive()? {
                    Response::Written { .. } => Ok(()),
                    Response::Error { code, .. } => Err(rpc_error_to_wasi(code)),
                    _ => Err(ErrorCode::Io),
                }
            })
        };

        // Advance offset after successful write
        if result.is_ok() && !self.append {
            self.offset.set(start_offset + data_len as u64);
        }

        result
    }
}

impl Drop for FileOutputStream {
    fn drop(&mut self) {
        let _ = self.flush_buffer(); // Ignore errors on drop
                                     // `stream_closed` runs while this Rc's strong count is already
                                     // zero, so the registry sees the stream as gone.
        let close_fd = RPC_STATE
            .try_with(|cell| {
                cell.borrow()
                    .as_ref()
                    .and_then(|state| state.stream_closed(self.handle))
            })
            .ok()
            .flatten();
        if let Some(fd) = close_fd {
            send_close(fd);
        }
    }
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
        // Just buffer the data - actual write happens on flush/drop.
        // Do NOT advance offset here; flush_buffer reads offset as the
        // start position and advances it after the write completes.
        self.buffer.borrow_mut().extend(contents);

        Ok(())
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        Ok(())
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        Ok(())
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        // Return an always-ready pollable since RPC is blocking
        exports::wasi::io::poll::Pollable::new(UnifiedPollable::AlwaysReady)
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
        Err(exports::wasi::io::streams::StreamError::Closed)
    }

    fn blocking_splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(exports::wasi::io::streams::StreamError::Closed)
    }
}

// Dummy pollable that's always ready (avoids nested runtime issue with WASI imports)
// Single representation type for the exported pollable resource. wit-bindgen
// tags the resource with the first Rust type passed to `Pollable::new` and
// panics ("cannot use two types with this resource type") if another type
// shows up later, so every pollable must go through this enum.
enum UnifiedPollable {
    // File-backed streams: RPC operations block at I/O level, always ready
    AlwaysReady,
    // Wraps a host pollable (passthrough streams, monotonic-clock timers)
    Passthrough(wasi::io::poll::Pollable),
}

impl exports::wasi::io::poll::GuestPollable for UnifiedPollable {
    fn ready(&self) -> bool {
        match self {
            UnifiedPollable::AlwaysReady => true,
            UnifiedPollable::Passthrough(inner) => inner.ready(),
        }
    }

    fn block(&self) {
        match self {
            UnifiedPollable::AlwaysReady => {}
            UnifiedPollable::Passthrough(inner) => inner.block(),
        }
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
    type Pollable = UnifiedPollable;

    fn poll(pollables: Vec<exports::wasi::io::poll::PollableBorrow<'_>>) -> Vec<u32> {
        // Always-ready entries resolve immediately. If the list is entirely
        // host-backed pollables, delegate the blocking wait to the host so
        // timers and stream readiness behave correctly.
        let mut ready: Vec<u32> = Vec::new();
        let mut host: Vec<(u32, &wasi::io::poll::Pollable)> = Vec::new();
        for (i, p) in pollables.iter().enumerate() {
            match p.get::<UnifiedPollable>() {
                UnifiedPollable::AlwaysReady => ready.push(i as u32),
                UnifiedPollable::Passthrough(inner) => host.push((i as u32, inner)),
            }
        }
        if ready.is_empty() && !host.is_empty() {
            let inners: Vec<&wasi::io::poll::Pollable> = host.iter().map(|(_, p)| *p).collect();
            return wasi::io::poll::poll(&inners)
                .into_iter()
                .map(|j| host[j as usize].0)
                .collect();
        }
        ready.extend(host.iter().filter(|(_, p)| p.ready()).map(|(i, _)| *i));
        ready
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
        exports::wasi::io::poll::Pollable::new(UnifiedPollable::Passthrough(inner))
    }

    fn subscribe_duration(
        when: exports::wasi::clocks::monotonic_clock::Duration,
    ) -> exports::wasi::clocks::monotonic_clock::Pollable {
        let inner = wasi::clocks::monotonic_clock::subscribe_duration(when);
        exports::wasi::io::poll::Pollable::new(UnifiedPollable::Passthrough(inner))
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
            UnifiedInputStream::Passthrough(p) => p
                .read(len)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.blocking_read(len),
            UnifiedInputStream::Passthrough(p) => p
                .blocking_read(len)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.skip(len),
            UnifiedInputStream::Passthrough(p) => p
                .skip(len)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn blocking_skip(&self, len: u64) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedInputStream::File(f) => f.blocking_skip(len),
            UnifiedInputStream::Passthrough(p) => p
                .blocking_skip(len)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        match self {
            UnifiedInputStream::File(f) => f.subscribe(),
            UnifiedInputStream::Passthrough(p) => {
                exports::wasi::io::poll::Pollable::new(UnifiedPollable::Passthrough(p.subscribe()))
            }
        }
    }
}

enum UnifiedOutputStream {
    File(Rc<FileOutputStream>),
    Passthrough(wasi::io::streams::OutputStream),
}

impl exports::wasi::io::streams::GuestOutputStream for UnifiedOutputStream {
    fn check_write(&self) -> Result<u64, exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.check_write(),
            UnifiedOutputStream::Passthrough(p) => p
                .check_write()
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn write(&self, contents: Vec<u8>) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.write(contents),
            UnifiedOutputStream::Passthrough(p) => p
                .write(&contents)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn blocking_write_and_flush(
        &self,
        contents: Vec<u8>,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.blocking_write_and_flush(contents),
            UnifiedOutputStream::Passthrough(p) => p
                .blocking_write_and_flush(&contents)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.flush(),
            UnifiedOutputStream::Passthrough(p) => p
                .flush()
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn blocking_flush(&self) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.blocking_flush(),
            UnifiedOutputStream::Passthrough(p) => p
                .blocking_flush()
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn subscribe(&self) -> exports::wasi::io::poll::Pollable {
        match self {
            UnifiedOutputStream::File(f) => f.subscribe(),
            UnifiedOutputStream::Passthrough(p) => {
                exports::wasi::io::poll::Pollable::new(UnifiedPollable::Passthrough(p.subscribe()))
            }
        }
    }

    fn write_zeroes(&self, len: u64) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.write_zeroes(len),
            UnifiedOutputStream::Passthrough(p) => p
                .write_zeroes(len)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn blocking_write_zeroes_and_flush(
        &self,
        len: u64,
    ) -> Result<(), exports::wasi::io::streams::StreamError> {
        match self {
            UnifiedOutputStream::File(f) => f.blocking_write_zeroes_and_flush(len),
            UnifiedOutputStream::Passthrough(p) => p
                .blocking_write_zeroes_and_flush(len)
                .map_err(|_| exports::wasi::io::streams::StreamError::Closed),
        }
    }

    fn splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(exports::wasi::io::streams::StreamError::Closed)
    }

    fn blocking_splice(
        &self,
        _src: exports::wasi::io::streams::InputStreamBorrow<'_>,
        _len: u64,
    ) -> Result<u64, exports::wasi::io::streams::StreamError> {
        Err(exports::wasi::io::streams::StreamError::Closed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_adds_leading_slash() {
        assert_eq!(normalize_path("foo/bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_keeps_single_leading_slash() {
        assert_eq!(normalize_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_collapses_repeated_leading_slashes() {
        assert_eq!(normalize_path("///foo"), "/foo");
    }

    #[test]
    fn normalize_path_handles_empty() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn rpc_error_maps_lock_codes() {
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::Busy),
            ErrorCode::Busy
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::Conflict),
            ErrorCode::NotRecoverable
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::NetworkError),
            ErrorCode::Io
        ));
    }

    #[test]
    fn busy_backoff_schedule_is_bounded_by_budget() {
        let s = busy_backoff_schedule(1000);
        assert_eq!(s.iter().sum::<u64>(), 1000);
        assert_eq!(s[0], 20);
        assert!(s.iter().all(|d| *d <= 500));
        assert!(busy_backoff_schedule(0).is_empty());
    }

    #[test]
    fn descriptor_drop_without_streams_releases_immediately() {
        let state = RpcState::new();
        let h = state.allocate_descriptor(7, 0x41);
        assert_eq!(state.descriptor_flags.borrow()[&h], 0x41);
        assert_eq!(state.descriptor_dropped(h), Some(7));
        assert!(state.get_server_fd(h).is_err());
    }

    #[test]
    fn descriptor_drop_with_live_stream_defers_close_to_stream_drop() {
        let state = RpcState::new();
        let h = state.allocate_descriptor(9, 0x1);
        let stream = Rc::new(FileOutputStream::new(h, 0, true));
        state.stream_opened(h, Rc::downgrade(&stream));

        assert_eq!(state.descriptor_dropped(h), None);
        assert!(state.get_server_fd(h).is_ok(), "mapping kept until flush");

        // Simulate the stream going away: drop the strong ref, then run the
        // bookkeeping the stream's Drop would run.
        let weak = Rc::downgrade(&stream);
        drop(stream);
        assert_eq!(weak.strong_count(), 0);
        assert_eq!(state.stream_closed(h), Some(9));
        assert!(state.get_server_fd(h).is_err());
    }

    #[test]
    fn stream_drop_before_descriptor_drop_does_not_close_early() {
        let state = RpcState::new();
        let h = state.allocate_descriptor(3, 0x1);
        let stream = Rc::new(FileOutputStream::new(h, 0, false));
        state.stream_opened(h, Rc::downgrade(&stream));
        drop(stream);
        assert_eq!(state.stream_closed(h), None);
        assert!(state.get_server_fd(h).is_ok());
        assert_eq!(state.descriptor_dropped(h), Some(3));
    }

    #[test]
    fn root_descriptor_is_never_released() {
        let state = RpcState::new();
        assert_eq!(state.get_server_fd(0).unwrap(), 0);
    }

    #[test]
    fn rpc_error_maps_known_codes() {
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::NotFound),
            ErrorCode::NoEntry
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::NotADirectory),
            ErrorCode::NotDirectory
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::IsADirectory),
            ErrorCode::IsDirectory
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::PermissionDenied),
            ErrorCode::Access
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::AlreadyExists),
            ErrorCode::Exist
        ));
        assert!(matches!(
            rpc_error_to_wasi(RpcErrorCode::NotEmpty),
            ErrorCode::NotEmpty
        ));
    }

    #[test]
    fn make_descriptor_stat_sets_directory_type() {
        let stat = make_descriptor_stat(true, 4096);
        assert!(matches!(stat.type_, DescriptorType::Directory));
        assert_eq!(stat.size, 4096);
        assert_eq!(stat.link_count, 1);
        assert!(stat.data_access_timestamp.is_none());
        assert!(stat.data_modification_timestamp.is_none());
        assert!(stat.status_change_timestamp.is_none());
    }

    #[test]
    fn make_descriptor_stat_sets_regular_file_type() {
        let stat = make_descriptor_stat(false, 123);
        assert!(matches!(stat.type_, DescriptorType::RegularFile));
        assert_eq!(stat.size, 123);
    }

    #[test]
    fn convert_flags_default_is_rdonly() {
        let f = convert_flags(OpenFlags::empty(), DescriptorFlags::READ);
        assert_eq!(f & 0x03, 0x00); // O_RDONLY
        assert_eq!(f & 0x40, 0); // no O_CREAT
        assert_eq!(f & 0x200, 0); // no O_TRUNC
    }

    #[test]
    fn convert_flags_write_only() {
        let f = convert_flags(OpenFlags::empty(), DescriptorFlags::WRITE);
        assert_eq!(f & 0x03, 0x01); // O_WRONLY
    }

    #[test]
    fn convert_flags_read_write() {
        let f = convert_flags(
            OpenFlags::empty(),
            DescriptorFlags::READ | DescriptorFlags::WRITE,
        );
        assert_eq!(f & 0x03, 0x02); // O_RDWR
    }

    #[test]
    fn convert_flags_create_and_trunc() {
        let f = convert_flags(
            OpenFlags::CREATE | OpenFlags::TRUNCATE,
            DescriptorFlags::WRITE,
        );
        assert_eq!(f & 0x40, 0x40); // O_CREAT
        assert_eq!(f & 0x200, 0x200); // O_TRUNC
    }
}
