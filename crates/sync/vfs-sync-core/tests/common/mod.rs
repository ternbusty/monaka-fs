//! Shared helpers for the sync manager tests: an in-memory `FsBackend` and
//! constructors for managers that share one `MemoryObjectStore`, which
//! models several instances syncing one bucket.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vfs_sync_core::testing::MemoryObjectStore;
use vfs_sync_core::{FsBackend, MetadataCache, S3Error, SyncConfig, SyncManager, SyncMode};

#[derive(Default)]
struct MemFsInner {
    files: HashMap<String, Vec<u8>>,
    mtimes: HashMap<String, u64>,
    dirs: HashSet<String>,
    fds: HashMap<u32, (String, usize)>,
    next_fd: u32,
    clock: u64,
}

/// In-memory filesystem good enough for whole-file reads and writes.
#[derive(Default, Clone)]
pub struct MemFs {
    inner: Arc<Mutex<MemFsInner>>,
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write as an application would: replace content, bump mtime.
    pub fn write_local(&self, path: &str, data: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let clock = inner.clock;
        inner.files.insert(path.to_string(), data.to_vec());
        inner.mtimes.insert(path.to_string(), clock);
    }

    pub fn append_local(&self, path: &str, data: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let clock = inner.clock;
        inner
            .files
            .entry(path.to_string())
            .or_default()
            .extend_from_slice(data);
        inner.mtimes.insert(path.to_string(), clock);
    }

    pub fn read_local(&self, path: &str) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().files.get(path).cloned()
    }

    pub fn remove_local(&self, path: &str) {
        self.inner.lock().unwrap().files.remove(path);
    }

    pub fn has_dir(&self, path: &str) -> bool {
        self.inner.lock().unwrap().dirs.contains(path)
    }

    pub fn file_paths(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inner.lock().unwrap().files.keys().cloned().collect();
        v.sort();
        v
    }

    fn err(kind: &str, key: &str) -> S3Error {
        S3Error::Read {
            key: key.to_string(),
            message: kind.to_string(),
        }
    }
}

impl FsBackend for MemFs {
    fn open_read(&self, path: &str) -> Result<u32, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.files.contains_key(path) {
            return Err(Self::err("not found", path));
        }
        inner.next_fd += 1;
        let fd = inner.next_fd;
        inner.fds.insert(fd, (path.to_string(), 0));
        Ok(fd)
    }

    fn open_write_truncate(&self, path: &str) -> Result<u32, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner.clock += 1;
        let clock = inner.clock;
        inner.files.insert(path.to_string(), Vec::new());
        inner.mtimes.insert(path.to_string(), clock);
        inner.next_fd += 1;
        let fd = inner.next_fd;
        inner.fds.insert(fd, (path.to_string(), 0));
        Ok(fd)
    }

    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        let (path, pos) = inner
            .fds
            .get(&fd)
            .cloned()
            .ok_or_else(|| Self::err("bad fd", ""))?;
        let data = inner.files.get(&path).cloned().unwrap_or_default();
        let n = buf.len().min(data.len().saturating_sub(pos));
        buf[..n].copy_from_slice(&data[pos..pos + n]);
        inner.fds.insert(fd, (path, pos + n));
        Ok(n)
    }

    fn write(&self, fd: u32, buf: &[u8]) -> Result<usize, S3Error> {
        let mut inner = self.inner.lock().unwrap();
        let (path, pos) = inner
            .fds
            .get(&fd)
            .cloned()
            .ok_or_else(|| Self::err("bad fd", ""))?;
        let file = inner.files.entry(path.clone()).or_default();
        if file.len() < pos + buf.len() {
            file.resize(pos + buf.len(), 0);
        }
        file[pos..pos + buf.len()].copy_from_slice(buf);
        inner.fds.insert(fd, (path, pos + buf.len()));
        Ok(buf.len())
    }

    fn close(&self, fd: u32) -> Result<(), S3Error> {
        self.inner.lock().unwrap().fds.remove(&fd);
        Ok(())
    }

    fn stat_modified(&self, path: &str) -> u64 {
        self.inner
            .lock()
            .unwrap()
            .mtimes
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    fn fstat_size(&self, fd: u32) -> Result<u64, S3Error> {
        let inner = self.inner.lock().unwrap();
        let (path, _) = inner.fds.get(&fd).ok_or_else(|| Self::err("bad fd", ""))?;
        Ok(inner.files.get(path).map(|d| d.len() as u64).unwrap_or(0))
    }

    fn unlink(&self, path: &str) -> Result<(), S3Error> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .files
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| Self::err("not found", path))
    }

    fn mkdir_p(&self, path: &str) {
        let mut inner = self.inner.lock().unwrap();
        let mut acc = String::new();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            acc.push('/');
            acc.push_str(seg);
            inner.dirs.insert(acc.clone());
        }
    }

    fn rmdir(&self, path: &str) -> Result<(), S3Error> {
        let mut inner = self.inner.lock().unwrap();
        let prefix = format!("{}/", path);
        if inner.files.keys().any(|k| k.starts_with(&prefix))
            || inner.dirs.iter().any(|d| d.starts_with(&prefix))
        {
            return Err(Self::err("not empty", path));
        }
        if inner.dirs.remove(path) {
            Ok(())
        } else {
            Err(Self::err("not found", path))
        }
    }
}

pub type Mgr = SyncManager<MemFs, MemoryObjectStore>;

pub fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Config with no environment influence, batch mode, and every interval
/// set to zero so `maybe_sync` flushes and polls on every call.
pub fn base_config() -> SyncConfig {
    let mut c = SyncConfig::from_lookup(|_| None);
    c.mode = SyncMode::Batch;
    c.poll_interval = Duration::ZERO;
    c.flush_interval = Duration::ZERO;
    c.outbound_batch_size = 10;
    c.file_lock = true;
    c.lock_timeout = Duration::from_millis(200);
    c.lock_lease = Duration::from_secs(30);
    c
}

pub fn store() -> Arc<MemoryObjectStore> {
    Arc::new(MemoryObjectStore::new())
}

pub fn instance(store: &Arc<MemoryObjectStore>, config: SyncConfig) -> (Mgr, MemFs) {
    let fs = MemFs::new();
    let mgr = SyncManager::new(store.clone(), fs.clone(), MetadataCache::new(), config);
    (mgr, fs)
}

/// Two managers over one store, each with its own local filesystem.
pub fn two_instances(
    store: &Arc<MemoryObjectStore>,
    config: SyncConfig,
) -> ((Mgr, MemFs), (Mgr, MemFs)) {
    (instance(store, config.clone()), instance(store, config))
}
