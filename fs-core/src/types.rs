/// File descriptor type
pub type Fd = u32;

/// Inode identifier type
pub type InodeId = u64;

/// File open flags
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0x40; // 64 - Create file if it doesn't exist
pub const O_TRUNC: u32 = 0x200; // 512 - Truncate file to 0 bytes on open
pub const O_APPEND: u32 = 0x400; // 1024 - Append mode: writes go to end of file

/// Block size for storage (4KB)
pub const BLOCK_SIZE: usize = 4096;
