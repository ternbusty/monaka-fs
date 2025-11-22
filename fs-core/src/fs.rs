use alloc::{collections::BTreeMap, rc::Rc};
use core::cell::RefCell;

use crate::error::FsError;
use crate::handle::FileHandle;
use crate::inode::{FileContent, Inode, Metadata};
use crate::time::{MonotonicCounter, TimeProvider};
use crate::types::*;

/// Main filesystem structure
pub struct Fs<T: TimeProvider = MonotonicCounter> {
    next_inode: InodeId,
    fd_table: BTreeMap<Fd, FileHandle>,
    inode_table: BTreeMap<InodeId, Rc<RefCell<Inode>>>,
    root_inode: InodeId,
    time_provider: T,
}

impl<T: TimeProvider> Fs<T> {
    pub fn with_time_provider(time_provider: T) -> Self {
        let mut fs = Self {
            next_inode: 1,
            fd_table: BTreeMap::new(),
            inode_table: BTreeMap::new(),
            root_inode: 0,
            time_provider,
        };

        // Create root directory inode with proper timestamps
        let timestamp = fs.time_provider.now();
        let mut root_inode = Inode::new_dir(0);
        root_inode.metadata.created = timestamp;
        root_inode.metadata.modified = timestamp;

        let root = Rc::new(RefCell::new(root_inode));
        fs.inode_table.insert(0, root);
        fs.root_inode = 0;

        fs
    }

    fn allocate_inode(&mut self) -> InodeId {
        let id = self.next_inode;
        self.next_inode += 1;
        id
    }

    fn allocate_fd(&mut self) -> Fd {
        // Find the lowest available file descriptor starting from 3
        // This implements POSIX-compliant FD reuse
        let mut fd = 3;
        while self.fd_table.contains_key(&fd) {
            fd += 1;
        }
        fd
    }

    fn find_inode(
        &self,
        parent_inode: &Rc<RefCell<Inode>>,
        name: &str,
    ) -> Result<Rc<RefCell<Inode>>, FsError> {
        let parent = parent_inode.borrow();

        match &parent.content {
            FileContent::Dir(entries) => {
                if let Some(&inode_id) = entries.get(name) {
                    if let Some(inode) = self.inode_table.get(&inode_id) {
                        Ok(inode.clone())
                    } else {
                        Err(FsError::NotFound)
                    }
                } else {
                    Err(FsError::NotFound)
                }
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    fn create_inode(
        &mut self,
        parent_inode: &Rc<RefCell<Inode>>,
        name: &str,
        is_dir: bool,
    ) -> Result<Rc<RefCell<Inode>>, FsError> {
        let mut parent = parent_inode.borrow_mut();

        match &mut parent.content {
            FileContent::Dir(entries) => {
                // Check if already exists
                if entries.contains_key(name) {
                    return Err(FsError::AlreadyExists);
                }

                // Create new inode
                let new_inode_id = self.allocate_inode();
                let timestamp = self.time_provider.now();
                let mut new_inode = if is_dir {
                    Inode::new_dir(new_inode_id)
                } else {
                    Inode::new_file(new_inode_id)
                };

                new_inode.metadata.created = timestamp;
                new_inode.metadata.modified = timestamp;

                let new_inode_rc = Rc::new(RefCell::new(new_inode));
                entries.insert(name.into(), new_inode_id);
                self.inode_table.insert(new_inode_id, new_inode_rc.clone());

                // Update parent directory's modification time
                parent.metadata.modified = timestamp;

                Ok(new_inode_rc)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }

    pub fn open_path(&mut self, path: &str) -> Result<Fd, FsError> {
        self.open_path_with_flags(path, O_RDWR | O_CREAT)
    }

    pub fn open_path_with_flags(&mut self, path: &str, flags: u32) -> Result<Fd, FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Handle final component (file name)
        let filename = comps[comps.len() - 1];
        let file_inode = match self.find_inode(&current_inode, filename) {
            Ok(inode) => inode,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                // Create new file if O_CREAT is set
                self.create_inode(&current_inode, filename, false)?
            }
            Err(e) => return Err(e),
        };

        // Verify it's a file, not a directory
        {
            let inode = file_inode.borrow();
            if matches!(inode.content, FileContent::Dir(_)) {
                return Err(FsError::IsADirectory);
            }
        }

        // Handle O_TRUNC: truncate file to 0 bytes if flag is set
        // POSIX requires write permission for O_TRUNC
        if flags & O_TRUNC != 0 {
            let access_mode = flags & 0x3;
            if access_mode == O_RDONLY {
                return Err(FsError::InvalidArgument);
            }
            let mut inode = file_inode.borrow_mut();
            if let FileContent::File(storage) = &mut inode.content {
                storage.truncate(0);
                inode.metadata.size = 0;
                inode.metadata.modified = self.time_provider.now();
            }
        }

        let inode_id = file_inode.borrow().id;
        let handle = FileHandle {
            inode_id,
            position: 0,
            flags,
        };

        let fd = self.allocate_fd();
        self.fd_table.insert(fd, handle);
        Ok(fd)
    }

    pub fn write(&mut self, fd: Fd, buf: &[u8]) -> Result<usize, FsError> {
        let handle = self.fd_table.get_mut(&fd).ok_or(FsError::BadFileDescriptor)?;

        // Check write permission
        let access_mode = handle.flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode.borrow_mut();

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                // Handle O_APPEND: move position to end of file before writing
                let pos = if handle.flags & O_APPEND != 0 {
                    let file_size = storage.size();
                    handle.position = file_size as u64;
                    file_size
                } else {
                    handle.position as usize
                };

                let n = storage.write(pos, buf);
                handle.position += n as u64;
                inode_ref.metadata.size = storage.size() as u64;
                inode_ref.metadata.modified = self.time_provider.now();
                Ok(n)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn read(&mut self, fd: Fd, out: &mut [u8]) -> Result<usize, FsError> {
        let handle = self.fd_table.get_mut(&fd).ok_or(FsError::BadFileDescriptor)?;

        // Check read permission
        let access_mode = handle.flags & 0x3;
        if access_mode == O_WRONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;
        let inode_ref = inode.borrow();

        match &inode_ref.content {
            FileContent::File(storage) => {
                let pos = handle.position as usize;
                let n = storage.read(pos, out);
                handle.position += n as u64;
                Ok(n)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn ftruncate(&mut self, fd: Fd, size: u64) -> Result<(), FsError> {
        let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;

        // Check write permission (truncate requires write access)
        let access_mode = handle.flags & 0x3;
        if access_mode == O_RDONLY {
            return Err(FsError::PermissionDenied);
        }

        let inode = self.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;
        let mut inode_ref = inode.borrow_mut();

        match &mut inode_ref.content {
            FileContent::File(storage) => {
                storage.truncate(size as usize);
                inode_ref.metadata.size = size;
                inode_ref.metadata.modified = self.time_provider.now();
                Ok(())
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn close(&mut self, fd: Fd) -> Result<(), FsError> {
        self.fd_table.remove(&fd).ok_or(FsError::BadFileDescriptor)?;
        Ok(())
    }

    pub fn stat(&self, path: &str) -> Result<Metadata, FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            // Root directory
            let root = self.inode_table.get(&self.root_inode).ok_or(FsError::NotFound)?;
            return Ok(root.borrow().metadata);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate through all components
        for comp in comps.iter() {
            current_inode = self.find_inode(&current_inode, comp)?;
        }

        Ok(current_inode.borrow().metadata)
    }

    pub fn fstat(&self, fd: Fd) -> Result<Metadata, FsError> {
        let handle = self.fd_table.get(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = self.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;
        Ok(inode.borrow().metadata)
    }

    pub fn seek(&mut self, fd: Fd, offset: i64, whence: i32) -> Result<u64, FsError> {
        const SEEK_SET: i32 = 0;
        const SEEK_CUR: i32 = 1;
        const SEEK_END: i32 = 2;

        let handle = self.fd_table.get_mut(&fd).ok_or(FsError::BadFileDescriptor)?;
        let inode = self.inode_table.get(&handle.inode_id).ok_or(FsError::NotFound)?;
        let inode_ref = inode.borrow();

        match &inode_ref.content {
            FileContent::File(storage) => {
                let new_pos = match whence {
                    SEEK_SET => {
                        if offset < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        offset as u64
                    }
                    SEEK_CUR => {
                        let new_pos_signed = handle.position as i64 + offset;
                        if new_pos_signed < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        new_pos_signed as u64
                    }
                    SEEK_END => {
                        let new_pos_signed = storage.size() as i64 + offset;
                        if new_pos_signed < 0 {
                            return Err(FsError::InvalidArgument);
                        }
                        new_pos_signed as u64
                    }
                    _ => return Err(FsError::InvalidArgument),
                };

                handle.position = new_pos;
                Ok(new_pos)
            }
            FileContent::Dir(_) => Err(FsError::BadFileDescriptor),
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Create the final directory component
        let dirname = comps[comps.len() - 1];
        self.create_inode(&current_inode, dirname, true)?;
        Ok(())
    }

    pub fn mkdir_p(&mut self, path: &str) -> Result<(), FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            return Ok(()); // Root already exists
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Create all components as needed
        for comp in comps.iter() {
            current_inode = match self.find_inode(&current_inode, comp) {
                Ok(inode) => {
                    // Verify it's a directory
                    {
                        let borrowed = inode.borrow();
                        if matches!(borrowed.content, FileContent::File(_)) {
                            return Err(FsError::NotADirectory);
                        }
                    }
                    inode
                }
                Err(FsError::NotFound) => {
                    // Create directory
                    self.create_inode(&current_inode, comp, true)?
                }
                Err(e) => return Err(e),
            };
        }

        Ok(())
    }

    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if comps.is_empty() {
            // Cannot unlink root directory
            return Err(FsError::InvalidArgument);
        }

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate to parent directory (all components except last)
        for comp in comps.iter().take(comps.len() - 1) {
            current_inode = self.find_inode(&current_inode, comp)?;

            // Verify it's a directory
            let inode = current_inode.borrow();
            if matches!(inode.content, FileContent::File(_)) {
                return Err(FsError::NotADirectory);
            }
        }

        // Get the filename to delete
        let filename = comps[comps.len() - 1];

        // Check if the file exists and get its inode
        let target_inode = self.find_inode(&current_inode, filename)?;

        // Check if it's a directory - use IsADirectory error
        {
            let target = target_inode.borrow();
            if matches!(target.content, FileContent::Dir(_)) {
                return Err(FsError::IsADirectory);
            }
        }

        // Remove from parent directory
        let mut parent = current_inode.borrow_mut();
        match &mut parent.content {
            FileContent::Dir(entries) => {
                entries.remove(filename);

                // Update parent directory's modification time
                let timestamp = self.time_provider.now();
                parent.metadata.modified = timestamp;

                Ok(())
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
        // Note: The inode will be automatically cleaned up when the Rc ref count reaches 0
    }

    pub fn readdir(&self, path: &str) -> Result<alloc::vec::Vec<alloc::string::String>, FsError> {
        if path.is_empty() {
            return Err(FsError::InvalidArgument);
        }

        let comps: alloc::vec::Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let root_inode = self
            .inode_table
            .get(&self.root_inode)
            .ok_or(FsError::NotFound)?
            .clone();
        let mut current_inode = root_inode;

        // Navigate through all components
        for comp in comps.iter() {
            current_inode = self.find_inode(&current_inode, comp)?;
        }

        // Check if it's a directory
        let inode = current_inode.borrow();
        match &inode.content {
            FileContent::Dir(entries) => {
                let mut result = alloc::vec::Vec::new();
                for name in entries.keys() {
                    result.push(name.clone());
                }
                Ok(result)
            }
            FileContent::File(_) => Err(FsError::NotADirectory),
        }
    }
}

impl Fs<MonotonicCounter> {
    pub fn new() -> Self {
        Self::with_time_provider(MonotonicCounter::new())
    }
}
