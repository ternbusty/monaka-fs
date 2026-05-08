//! Round-trip and edge-case tests for the wire protocol.
//!
//! These tests pin down two things:
//! 1. Every `Request` and `Response` variant survives the encode → decode
//!    round trip with field values intact. A protobuf field tag mistake or
//!    a missing variant in `from_proto_request` / `from_proto_response_bytes`
//!    will trip this immediately.
//! 2. Edge cases for `ErrorCode` conversion (every variant, unknown values).

use prost::Message;
use vfs_rpc_protocol::{
    from_proto_request, from_proto_response_bytes, to_proto_request_bytes, to_proto_response, vfs,
    DirEntry, ErrorCode, FileMetadata, Request, Response, RpcRequestMessage,
};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Mirrors what the rpc-server does on the wire: serialize the client-side
/// `RpcRequestMessage` to bytes, decode with `prost`, and convert back to
/// the internal representation.
fn request_round_trip(msg: &RpcRequestMessage) -> RpcRequestMessage {
    let bytes = to_proto_request_bytes(msg);
    let proto = vfs::RpcRequest::decode(&bytes[..]).expect("decode RpcRequest");
    from_proto_request(proto).expect("from_proto_request")
}

/// Mirrors what the rpc-adapter does on the wire: convert a server-side
/// `Response` to protobuf, encode to bytes, and parse back.
fn response_round_trip(resp: Response) -> Response {
    let proto = to_proto_response(resp);
    let bytes = proto.encode_to_vec();
    from_proto_response_bytes(&bytes).expect("from_proto_response_bytes")
}

fn rq(request: Request) -> RpcRequestMessage {
    RpcRequestMessage {
        session_id: Some("test-session".into()),
        request,
    }
}

// ---------------------------------------------------------------------------
// Request round-trips - one test per variant
// ---------------------------------------------------------------------------

#[test]
fn request_connect_round_trip() {
    let out = request_round_trip(&rq(Request::Connect { version: 42 }));
    assert_eq!(out.session_id.as_deref(), Some("test-session"));
    match out.request {
        Request::Connect { version } => assert_eq!(version, 42),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_connect_no_session() {
    // session_id should round-trip as `None` when not provided (e.g. the
    // initial handshake).
    let msg = RpcRequestMessage {
        session_id: None,
        request: Request::Connect { version: 1 },
    };
    let out = request_round_trip(&msg);
    assert_eq!(out.session_id, None);
}

#[test]
fn request_open_path_round_trip() {
    let out = request_round_trip(&rq(Request::OpenPath {
        path: "/foo/bar.txt".into(),
        flags: 0o102, // O_CREAT | O_RDWR
    }));
    match out.request {
        Request::OpenPath { path, flags } => {
            assert_eq!(path, "/foo/bar.txt");
            assert_eq!(flags, 0o102);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_open_at_round_trip() {
    let out = request_round_trip(&rq(Request::OpenAt {
        dir_fd: 7,
        path: "child.bin".into(),
        flags: 0,
    }));
    match out.request {
        Request::OpenAt {
            dir_fd,
            path,
            flags,
        } => {
            assert_eq!(dir_fd, 7);
            assert_eq!(path, "child.bin");
            assert_eq!(flags, 0);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_read_round_trip() {
    // Pick a length above u32::MAX to catch any narrowing in the
    // u64 ↔ usize conversion on 64-bit hosts.
    let big_len: usize = (u32::MAX as usize) + 17;
    let out = request_round_trip(&rq(Request::Read {
        fd: 3,
        length: big_len,
    }));
    match out.request {
        Request::Read { fd, length } => {
            assert_eq!(fd, 3);
            assert_eq!(length, big_len);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_write_round_trip() {
    let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    let out = request_round_trip(&rq(Request::Write {
        fd: 12,
        data: payload.clone(),
    }));
    match out.request {
        Request::Write { fd, data } => {
            assert_eq!(fd, 12);
            assert_eq!(data, payload);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_write_empty_data() {
    // A zero-length payload is a real wire case and must survive intact.
    let out = request_round_trip(&rq(Request::Write {
        fd: 1,
        data: Vec::new(),
    }));
    match out.request {
        Request::Write { fd, data } => {
            assert_eq!(fd, 1);
            assert!(data.is_empty());
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_close_round_trip() {
    let out = request_round_trip(&rq(Request::Close { fd: 9 }));
    match out.request {
        Request::Close { fd } => assert_eq!(fd, 9),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_seek_round_trip() {
    // Negative offsets are valid for SEEK_CUR / SEEK_END and must survive.
    let out = request_round_trip(&rq(Request::Seek {
        fd: 4,
        offset: -1024,
        whence: 2,
    }));
    match out.request {
        Request::Seek { fd, offset, whence } => {
            assert_eq!(fd, 4);
            assert_eq!(offset, -1024);
            assert_eq!(whence, 2);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_ftruncate_round_trip() {
    let out = request_round_trip(&rq(Request::Ftruncate {
        fd: 5,
        size: u64::MAX,
    }));
    match out.request {
        Request::Ftruncate { fd, size } => {
            assert_eq!(fd, 5);
            assert_eq!(size, u64::MAX);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_stat_round_trip() {
    let out = request_round_trip(&rq(Request::Stat {
        path: "/path/with spaces/and-unicode-α".into(),
    }));
    match out.request {
        Request::Stat { path } => assert_eq!(path, "/path/with spaces/and-unicode-α"),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_fstat_round_trip() {
    let out = request_round_trip(&rq(Request::Fstat { fd: 99 }));
    match out.request {
        Request::Fstat { fd } => assert_eq!(fd, 99),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_mkdir_round_trip() {
    let out = request_round_trip(&rq(Request::Mkdir { path: "/d".into() }));
    match out.request {
        Request::Mkdir { path } => assert_eq!(path, "/d"),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_mkdir_p_round_trip() {
    let out = request_round_trip(&rq(Request::MkdirP {
        path: "/a/b/c".into(),
    }));
    match out.request {
        Request::MkdirP { path } => assert_eq!(path, "/a/b/c"),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_unlink_round_trip() {
    let out = request_round_trip(&rq(Request::Unlink {
        path: "/tmp/file".into(),
    }));
    match out.request {
        Request::Unlink { path } => assert_eq!(path, "/tmp/file"),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_readdir_round_trip() {
    let out = request_round_trip(&rq(Request::Readdir { path: "/".into() }));
    match out.request {
        Request::Readdir { path } => assert_eq!(path, "/"),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_readdir_fd_round_trip() {
    let out = request_round_trip(&rq(Request::ReaddirFd { fd: 21 }));
    match out.request {
        Request::ReaddirFd { fd } => assert_eq!(fd, 21),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_rmdir_round_trip() {
    let out = request_round_trip(&rq(Request::Rmdir { path: "/d".into() }));
    match out.request {
        Request::Rmdir { path } => assert_eq!(path, "/d"),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_append_write_round_trip() {
    let out = request_round_trip(&rq(Request::AppendWrite {
        fd: 6,
        data: b"appended\n".to_vec(),
    }));
    match out.request {
        Request::AppendWrite { fd, data } => {
            assert_eq!(fd, 6);
            assert_eq!(data, b"appended\n");
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn request_rename_round_trip() {
    let out = request_round_trip(&rq(Request::Rename {
        old_path: "/old".into(),
        new_path: "/new/nested/path".into(),
    }));
    match out.request {
        Request::Rename { old_path, new_path } => {
            assert_eq!(old_path, "/old");
            assert_eq!(new_path, "/new/nested/path");
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Server-side parse failures
// ---------------------------------------------------------------------------

#[test]
fn from_proto_request_rejects_missing_request_oneof() {
    // A protobuf RpcRequest with `request` unset should be rejected.
    let proto = vfs::RpcRequest {
        session_id: Some("sess".into()),
        request: None,
    };
    let err = from_proto_request(proto).expect_err("must error");
    assert!(err.contains("Missing"), "unexpected error: {}", err);
}

// ---------------------------------------------------------------------------
// Response round-trips
// ---------------------------------------------------------------------------

#[test]
fn response_connected_round_trip() {
    let out = response_round_trip(Response::Connected {
        session_id: "abc123".into(),
        version: 1,
    });
    match out {
        Response::Connected {
            session_id,
            version,
        } => {
            assert_eq!(session_id, "abc123");
            assert_eq!(version, 1);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_ok_round_trip() {
    match response_round_trip(Response::Ok) {
        Response::Ok => {}
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_fd_round_trip() {
    match response_round_trip(Response::Fd { fd: 17 }) {
        Response::Fd { fd } => assert_eq!(fd, 17),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_data_round_trip() {
    let payload = (0..1024).map(|i| (i % 256) as u8).collect::<Vec<u8>>();
    match response_round_trip(Response::Data {
        bytes: payload.clone(),
    }) {
        Response::Data { bytes } => assert_eq!(bytes, payload),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_written_round_trip() {
    let big: usize = (u32::MAX as usize) + 1;
    match response_round_trip(Response::Written { count: big }) {
        Response::Written { count } => assert_eq!(count, big),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_position_round_trip() {
    match response_round_trip(Response::Position { pos: u64::MAX }) {
        Response::Position { pos } => assert_eq!(pos, u64::MAX),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_metadata_round_trip() {
    let meta = FileMetadata {
        size: 1024,
        created: 1_700_000_000,
        modified: 1_700_000_500,
        is_dir: false,
    };
    match response_round_trip(Response::Metadata {
        metadata: meta.clone(),
    }) {
        Response::Metadata { metadata } => {
            assert_eq!(metadata.size, meta.size);
            assert_eq!(metadata.created, meta.created);
            assert_eq!(metadata.modified, meta.modified);
            assert_eq!(metadata.is_dir, meta.is_dir);
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_dir_entries_round_trip() {
    let entries = vec![
        DirEntry {
            name: "file.txt".into(),
            is_dir: false,
        },
        DirEntry {
            name: "subdir".into(),
            is_dir: true,
        },
        DirEntry {
            name: "".into(),
            is_dir: false,
        },
    ];
    match response_round_trip(Response::DirEntries {
        entries: entries.clone(),
    }) {
        Response::DirEntries { entries: out } => {
            assert_eq!(out.len(), entries.len());
            for (a, b) in out.iter().zip(entries.iter()) {
                assert_eq!(a.name, b.name);
                assert_eq!(a.is_dir, b.is_dir);
            }
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_dir_entries_empty_round_trip() {
    match response_round_trip(Response::DirEntries { entries: vec![] }) {
        Response::DirEntries { entries } => assert!(entries.is_empty()),
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn response_error_round_trip_each_code() {
    let codes = [
        ErrorCode::NotFound,
        ErrorCode::NotADirectory,
        ErrorCode::IsADirectory,
        ErrorCode::InvalidArgument,
        ErrorCode::BadFileDescriptor,
        ErrorCode::PermissionDenied,
        ErrorCode::AlreadyExists,
        ErrorCode::NotEmpty,
        ErrorCode::NetworkError,
        ErrorCode::ProtocolError,
        ErrorCode::SerializationError,
        ErrorCode::Io,
    ];
    for code in codes {
        let msg = format!("err for {:?}", code);
        match response_round_trip(Response::Error {
            code,
            message: msg.clone(),
        }) {
            Response::Error {
                code: out_code,
                message,
            } => {
                assert_eq!(out_code, code);
                assert_eq!(message, msg);
            }
            other => panic!("unexpected variant for {:?}: {:?}", code, other),
        }
    }
}

#[test]
fn from_proto_response_bytes_rejects_garbage() {
    let err = from_proto_response_bytes(&[0xff, 0xff, 0xff, 0xff]).expect_err("must error");
    assert_eq!(err, ErrorCode::SerializationError);
}

#[test]
fn from_proto_response_bytes_rejects_empty_oneof() {
    // Encode a RpcResponse with `response` unset and verify we get
    // a SerializationError rather than a panic.
    let proto = vfs::RpcResponse { response: None };
    let bytes = proto.encode_to_vec();
    let err = from_proto_response_bytes(&bytes).expect_err("must error");
    assert_eq!(err, ErrorCode::SerializationError);
}

// ---------------------------------------------------------------------------
// ErrorCode utilities
// ---------------------------------------------------------------------------

#[test]
fn error_code_from_i32_recognises_every_value() {
    let pairs: &[(i32, ErrorCode)] = &[
        (1, ErrorCode::NotFound),
        (2, ErrorCode::NotADirectory),
        (3, ErrorCode::IsADirectory),
        (4, ErrorCode::InvalidArgument),
        (5, ErrorCode::BadFileDescriptor),
        (6, ErrorCode::PermissionDenied),
        (7, ErrorCode::AlreadyExists),
        (8, ErrorCode::NotEmpty),
        (9, ErrorCode::NetworkError),
        (10, ErrorCode::ProtocolError),
        (11, ErrorCode::SerializationError),
        (12, ErrorCode::Io),
    ];
    for (raw, expected) in pairs {
        assert_eq!(ErrorCode::from_i32(*raw), Some(*expected));
        // Re-cast through `as i32` to confirm the discriminant value
        // matches what we round-trip with.
        assert_eq!(*raw, *expected as i32);
    }
}

#[test]
fn error_code_from_i32_rejects_unknown_values() {
    assert_eq!(ErrorCode::from_i32(0), None);
    assert_eq!(ErrorCode::from_i32(-1), None);
    assert_eq!(ErrorCode::from_i32(99), None);
    assert_eq!(ErrorCode::from_i32(i32::MAX), None);
}

#[test]
fn error_code_unknown_payload_falls_back_to_io() {
    // Server sends an Error with a code value the client doesn't recognise.
    // The client should map it to `Io` rather than panic or drop the message.
    let proto = vfs::RpcResponse {
        response: Some(vfs::rpc_response::Response::Error(vfs::Error {
            code: 9999,
            message: "future code".into(),
        })),
    };
    let bytes = proto.encode_to_vec();
    match from_proto_response_bytes(&bytes).expect("decode") {
        Response::Error { code, message } => {
            assert_eq!(code, ErrorCode::Io);
            assert_eq!(message, "future code");
        }
        other => panic!("unexpected variant: {:?}", other),
    }
}

#[test]
fn error_code_as_str_non_empty() {
    for code in [
        ErrorCode::NotFound,
        ErrorCode::NotADirectory,
        ErrorCode::IsADirectory,
        ErrorCode::InvalidArgument,
        ErrorCode::BadFileDescriptor,
        ErrorCode::PermissionDenied,
        ErrorCode::AlreadyExists,
        ErrorCode::NotEmpty,
        ErrorCode::NetworkError,
        ErrorCode::ProtocolError,
        ErrorCode::SerializationError,
        ErrorCode::Io,
    ] {
        assert!(!code.as_str().is_empty(), "{:?} has empty as_str", code);
    }
}
