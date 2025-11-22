use alloc::{boxed::Box, vec::Vec};

use crate::types::BLOCK_SIZE;

pub struct BlockStorage {
    blocks: Vec<Option<Box<[u8; BLOCK_SIZE]>>>,
    size: usize, // Actual file size (may be less than blocks.len() * BLOCK_SIZE)
}

impl BlockStorage {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            size: 0,
        }
    }

    pub fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
        if offset >= self.size {
            return 0;
        }

        let mut bytes_read = 0;
        let mut current_offset = offset;

        while bytes_read < buf.len() && current_offset < self.size {
            let block_index = current_offset / BLOCK_SIZE;
            let block_offset = current_offset % BLOCK_SIZE;

            if block_index >= self.blocks.len() {
                break;
            }

            if let Some(block) = &self.blocks[block_index] {
                let bytes_in_block = BLOCK_SIZE - block_offset;
                let bytes_to_copy = core::cmp::min(
                    bytes_in_block,
                    core::cmp::min(buf.len() - bytes_read, self.size - current_offset),
                );

                buf[bytes_read..bytes_read + bytes_to_copy]
                    .copy_from_slice(&block[block_offset..block_offset + bytes_to_copy]);

                bytes_read += bytes_to_copy;
                current_offset += bytes_to_copy;
            } else {
                // Sparse block - fill with zeros
                let bytes_in_block = BLOCK_SIZE - block_offset;
                let bytes_to_copy = core::cmp::min(
                    bytes_in_block,
                    core::cmp::min(buf.len() - bytes_read, self.size - current_offset),
                );

                for i in 0..bytes_to_copy {
                    buf[bytes_read + i] = 0;
                }

                bytes_read += bytes_to_copy;
                current_offset += bytes_to_copy;
            }
        }

        bytes_read
    }

    pub fn write(&mut self, offset: usize, data: &[u8]) -> usize {
        if data.is_empty() {
            return 0;
        }

        let mut bytes_written = 0;
        let mut current_offset = offset;

        while bytes_written < data.len() {
            let block_index = current_offset / BLOCK_SIZE;
            let block_offset = current_offset % BLOCK_SIZE;

            // Ensure we have enough blocks
            while block_index >= self.blocks.len() {
                self.blocks.push(None);
            }

            // Allocate block if needed
            if self.blocks[block_index].is_none() {
                self.blocks[block_index] = Some(Box::new([0u8; BLOCK_SIZE]));
            }

            if let Some(block) = &mut self.blocks[block_index] {
                let bytes_in_block = BLOCK_SIZE - block_offset;
                let bytes_to_copy = core::cmp::min(bytes_in_block, data.len() - bytes_written);

                block[block_offset..block_offset + bytes_to_copy]
                    .copy_from_slice(&data[bytes_written..bytes_written + bytes_to_copy]);

                bytes_written += bytes_to_copy;
                current_offset += bytes_to_copy;
            }
        }

        // Update file size if we wrote past the end
        if current_offset > self.size {
            self.size = current_offset;
        }

        bytes_written
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn truncate(&mut self, new_size: usize) {
        if new_size < self.size {
            // Shrinking the file
            let new_block_count = (new_size + BLOCK_SIZE - 1) / BLOCK_SIZE;

            // Free blocks beyond the new size
            self.blocks.truncate(new_block_count);

            // Clear bytes beyond new_size in the last block (if partially filled)
            if new_size > 0 && new_block_count > 0 {
                let last_block_index = new_block_count - 1;
                let offset_in_last_block = new_size % BLOCK_SIZE;

                if offset_in_last_block > 0 {
                    if let Some(Some(block)) = self.blocks.get_mut(last_block_index) {
                        // Clear the remainder of the block
                        for byte in &mut block[offset_in_last_block..] {
                            *byte = 0;
                        }
                    }
                }
            }

            self.size = new_size;
        } else if new_size > self.size {
            // Expanding the file
            // Note: We don't need to allocate blocks immediately for sparse files
            // They will be allocated on write. Just update the size.
            self.size = new_size;
        }
        // If new_size == self.size, do nothing
    }
}
