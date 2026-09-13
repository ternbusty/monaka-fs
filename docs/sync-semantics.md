# S3 sync semantics

This document is the contract for Monaka's S3 synchronization layer. It states what the sync layer guarantees when one or more Monaka instances share an S3 bucket and prefix, what it does not guarantee, and how each failure is reported to an application. It is modelled on the semantics documents that Mountpoint for Amazon S3 and Cloud Storage FUSE publish for the same purpose.

The sync layer lives in `crates/sync/vfs-sync-core` and is used unchanged by all three deployment models.

| Model | Crate that embeds the sync layer |
|---|---|
| Static composition | `vfs-adapter` (feature `s3-sync`) via `vfs-sync-adapter` |
| RPC server | `vfs-rpc-server` (feature `s3-sync`) via `vfs-sync-adapter` |
| Host trait | `vfs-host` (feature `s3-sync`) via `vfs-sync-host` |

## Consistency model

S3 is the source of truth. Each instance keeps an in-memory copy of the files under its prefix and exchanges changes with S3 in both directions.

### Close to open

Writes become visible to other instances when the last write descriptor for the file is closed on the writing instance. Another instance that opens the same file for write afterwards observes that content. This matches the close-to-open model of NFS, Cloud Storage FUSE and goofys. Content written to a still-open descriptor is not visible elsewhere.

`sync` and `sync-data` (`File::sync_all` and `File::sync_data` in Rust) push the current content to S3 without closing the descriptor.

### Change detection

Inbound change detection compares ETags only. The ETag an instance last observed for a file is recorded in its metadata cache, and a listing entry with a different ETag triggers a download. Wall-clock timestamps are never compared, because S3's last-modified time and the local modification time come from different clocks.

Two inbound mechanisms exist and are selected by `VFS_INBOUND_MODE` and `VFS_READ_MODE`. Polling lists the prefix every `VFS_POLL_INTERVAL_SECS` and downloads changed files that this instance is not currently writing. Read-through fetches the file from S3 on the first read of each descriptor.

### Conditional writes

Every write to S3 is conditional. An existing object is replaced with `If-Match` on the ETag this instance last observed, a new object is created with `If-None-Match: *`, and a delete carries `If-Match` on the last observed ETag. S3 evaluates these conditions atomically against the current object, so a write based on stale state is rejected with HTTP 412 instead of overwriting another instance's change. The same applies to `CompleteMultipartUpload` for large files.

## Per-file leases

With `VFS_S3_FILE_LOCK=enabled` (the default), opening a file for write takes a per-file lease before the local open.

| Step | Request | Outcome |
|---|---|---|
| Acquire | `PutObject locks/<path>` with `If-None-Match: *` | Success means this instance holds the lease. A 412 means another instance holds it |
| Held by another instance, not expired | wait with backoff | Up to `VFS_S3_FILE_LOCK_TIMEOUT_MS`, then the open fails with a busy error |
| Held by another instance, expired | `PutObject locks/<path>` with `If-Match` on the stale record | Takeover, atomic. A 412 here means someone else took it first |
| Refresh | `HeadObject files/<path>`, then `GetObject` when the ETag differs | The local copy matches S3 before the application writes, and the ETag is pinned as the base for the close |
| Renew | `PutObject locks/<path>` with `If-Match` on the lease record | Performed from the background sync tick once half the lease has elapsed |
| Release | `PutObject files/<path>` with `If-Match` on the pinned base, then `DeleteObject locks/<path>` with `If-Match` on the lease record | Runs when the last write descriptor closes. The data write always precedes the lease delete |

The lease is held from open to the close of the last write descriptor on that instance and released as soon as the close-time write completes. Several descriptors on one instance share one lease. The expiry (`VFS_S3_FILE_LOCK_LEASE_SECS`) only matters when the holder stops renewing, which happens when it crashes or hangs. A live holder never loses its lease to expiry.

A file opened for read takes no lease. A file opened for write while this instance is polling is never overwritten by the poll.

### Lease record

The lease object body is plain text with one `key=value` per line. It carries the holder's instance id, an epoch that increases on every takeover, the expiry as Unix milliseconds, and the path. Expiry is wall-clock based because it has to be comparable across instances. Clock skew shifts the moment a takeover becomes possible but cannot cause data loss, because the data write is fenced by `If-Match` regardless of who believes they hold the lease.

### Fencing gap

A holder whose lease expired may still complete its close-time `PutObject` if the new holder has not written yet. The new holder's own write then fails with 412, its local copy is refreshed from S3, and the failure is reported as a conflict. No bytes are lost. The window is bounded by the lease duration and stays closed while the original holder is alive, because renewal runs from the background tick.

### Disabled leases

With `VFS_S3_FILE_LOCK=disabled` no lease objects are created and opens never wait. Writes stay conditional, so a concurrent change is still detected, but the outcome is the pre-lease behaviour of last writer wins. The rejected write is logged and retried unconditionally. Use this only when a single instance writes to the prefix.

## Guarantees

| Property | Leases enabled | Leases disabled |
|---|---|---|
| A write never silently overwrites another instance's newer content | Guaranteed | Not guaranteed, last writer wins |
| A file opened for write reflects the last close on any instance | Guaranteed | Only after the next poll |
| At most one instance writes a given file at a time | Guaranteed while the holder is alive | Not guaranteed |
| A deleted file does not reappear from a stale queue on another instance | Guaranteed | Guaranteed |
| Whole-object atomicity (readers never see a partial object) | Guaranteed by S3 | Guaranteed by S3 |
| Atomicity across several files | Not provided | Not provided |
| `rename` is reflected in S3 | Not provided | Not provided |

## Error reporting

The sync layer surfaces two conditions to applications. Everything else is logged and treated as an I/O error.

| Condition | `vfs-sync-core` | WASI `error-code` (static composition, host trait) | RPC `ErrorCode` | Rust `io::ErrorKind` |
|---|---|---|---|---|
| Lease held by another instance past the timeout | `SyncError::Busy` | `busy` | `Busy` | `ResourceBusy` |
| Write rejected by `If-Match` under a held lease (fencing gap) | `SyncError::Conflict` | `not-recoverable` | `Conflict` | `Other` (raw OS error 56) |

A busy error is returned from the open itself. A conflict is returned from `sync` and `sync-data`, and from a write in realtime mode. It cannot be returned from a close, because dropping a WASI descriptor resource cannot fail. An application that needs to observe conflicts should call `File::sync_all` before dropping the file. In every case the local copy has already been replaced with the S3 version, so re-reading the file and reapplying the change is the recovery path.

The RPC server never waits for a lease, because its event loop is single-threaded and a blocked handler would stall every session. It answers the open with `Busy` immediately and the `rpc-adapter` retries with backoff up to `VFS_S3_FILE_LOCK_TIMEOUT_MS` before reporting the error to the application.

## Directory markers

`mkdir` creates the empty object `files/<dir>/` with `If-None-Match: *`, and `rmdir` deletes it. Loading from S3 and polling create local directories for markers and remove local directories whose marker disappeared, provided they are empty. Directories that exist only implicitly, because a file lives under them, are never removed this way.

## Batch and realtime modes under a lease

`VFS_SYNC_MODE=batch` (the default) queues writes and flushes them every `VFS_FLUSH_INTERVAL_SECS` or `VFS_OUTBOUND_BATCH_SIZE` operations. A write to a leased file is not queued. It marks the lease dirty, and the close of the last write descriptor pushes the content once. The batch settings therefore only affect files written without a lease, such as files written while `VFS_S3_FILE_LOCK=disabled`.

`VFS_SYNC_MODE=realtime` writes to S3 on every write call. Under a lease each write carries `If-Match` on the current base and advances it. The close then has nothing left to push and only releases the lease.

## S3 object layout

All keys live under `VFS_S3_PREFIX`.

```
<prefix>files/<path>      file content
<prefix>files/<dir>/      directory marker (empty object)
<prefix>locks/<path>      lease record
```

Files can be read and written by other tools while Monaka runs. A write from another tool changes the object's ETag and is picked up by the next poll or open. Such tools do not take leases, so a concurrent write from them is detected by `If-Match` on the Monaka side and reported as a conflict.

## Environment variables

| Variable | Values | Default | Applies to |
|---|---|---|---|
| `VFS_S3_BUCKET` | bucket name | unset, sync disabled | all |
| `VFS_S3_PREFIX` | key prefix | `vfs/` | all |
| `VFS_SYNC_MODE` | `batch`, `realtime` | `batch` | all |
| `VFS_INBOUND_MODE` | `none`, `polling`, `readthrough` | `polling` | all |
| `VFS_READ_MODE` | `memory`, `s3` | `memory` | all |
| `VFS_METADATA_MODE` | `local`, `s3` | `local` | all |
| `VFS_POLL_INTERVAL_SECS` | seconds | `30` | all |
| `VFS_FLUSH_INTERVAL_SECS` | seconds | `5` | all |
| `VFS_OUTBOUND_BATCH_SIZE` | count | `10` | all |
| `VFS_S3_FILE_LOCK` | `enabled`, `disabled` | `enabled` | all |
| `VFS_S3_FILE_LOCK_TIMEOUT_MS` | milliseconds | `10000` | all, and the `rpc-adapter` retry budget |
| `VFS_S3_FILE_LOCK_LEASE_SECS` | seconds | `30` | all |
| `VFS_RPC_PORT` | TCP port | `9000` | `vfs-rpc-server` and `rpc-adapter` |
| `AWS_ENDPOINT_URL` | URL | unset | all |
| `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | | | all, read by the AWS SDK |

## Supported backends

The sync layer requires a backend that evaluates `If-Match` and `If-None-Match: *` on `PutObject` and `CompleteMultipartUpload`.

| Backend | Status |
|---|---|
| Amazon S3 | Supported. Conditional writes are generally available |
| LocalStack 4.0.3 up to 4.14.0 | Supported and used by the e2e suite. Conditional `DeleteObject` is answered with `NotImplemented`, and the sync layer falls back to a `HeadObject` comparison followed by an unconditional delete for that backend |
| LocalStack 2026.03.0 and later | Requires an auth token to start. Not used by CI |
| MinIO | Not supported. It rejects `If-None-Match: *`, which lease acquisition depends on |

`scripts/e2e.sh` probes the backend during pre-flight with the same three requests and refuses to run the S3 tiers against a backend that does not enforce them.

## Known gaps

* `rename` is applied locally only. The old and new paths are not reflected in S3 until the next write of the new path.
* Conditional delete depends on the backend, as described above. On a backend that rejects it, a delete that races with another instance's write is not atomic.
* Lease expiry uses the wall clock. Instances whose clocks disagree by more than a few seconds see takeovers earlier or later than intended, never lost data.
* There is no cross-instance notification. Other instances learn about a change on their next poll or on their next open for write.
* On the host trait model, a `VfsHostState` built with `from_shared_vfs_with_env` carries no sync hooks and therefore takes no leases.
* An RPC client that disconnects without closing its descriptors has them closed by the server on disconnect. A client that is still connected but has crashed mid-write holds its leases until they expire.
