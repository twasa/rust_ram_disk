use std::collections::HashMap;
use parking_lot::RwLock;
use thiserror::Error;

use super::block::{Block, CompressionMode};
use super::stats::StorageStats;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Offset {offset} is out of bounds for capacity {capacity}")]
    OutOfBounds { offset: u64, capacity: u64 },
    #[error("Block error: {0}")]
    BlockError(#[from] super::block::BlockError),
    #[error("I/O persistence error: {0}")]
    IoError(#[from] std::io::Error),
}

pub trait StorageEngine: Send + Sync {
    /// Total virtual capacity of the RAM disk in bytes.
    fn capacity(&self) -> u64;

    /// Block (sector/chunk) size in bytes (e.g., 4096 or 65536).
    fn block_size(&self) -> usize;

    /// Number of actively allocated physical blocks.
    fn allocated_blocks(&self) -> usize;

    /// Total bytes of physical RAM currently consumed.
    fn physical_memory_used(&self) -> usize;

    /// Read bytes from a byte offset. Unallocated sparse blocks read as zeros.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError>;

    /// Write bytes to a byte offset. Dynamically allocates blocks on demand.
    fn write_at(&self, offset: u64, buf: &[u8]) -> Result<usize, StorageError>;

    /// Flush / synchronize storage state.
    fn flush(&self) -> Result<(), StorageError>;
}

pub struct SparseMemoryStorage {
    capacity: u64,
    block_size: usize,
    compression: CompressionMode,
    blocks: RwLock<HashMap<u64, Block>>,
    stats: StorageStats,
}

impl SparseMemoryStorage {
    pub const DEFAULT_BLOCK_SIZE: usize = 4096; // 4 KB standard page/sector

    pub fn new(capacity: u64, block_size: usize, compression: CompressionMode) -> Self {
        assert!(block_size > 0, "Block size must be greater than 0");
        Self {
            capacity,
            block_size,
            compression,
            blocks: RwLock::new(HashMap::new()),
            stats: StorageStats::new(),
        }
    }

    pub fn stats(&self) -> &StorageStats {
        &self.stats
    }

    pub fn compression_mode(&self) -> CompressionMode {
        self.compression
    }

    /// Exposes read-only block map snapshot for persistence/dumping.
    pub fn get_allocated_block_indices(&self) -> Vec<u64> {
        let guard = self.blocks.read();
        guard.keys().copied().collect()
    }
}

impl StorageEngine for SparseMemoryStorage {
    fn capacity(&self) -> u64 {
        self.capacity
    }

    fn block_size(&self) -> usize {
        self.block_size
    }

    fn allocated_blocks(&self) -> usize {
        self.blocks.read().len()
    }

    fn physical_memory_used(&self) -> usize {
        let guard = self.blocks.read();
        guard.values().map(|b| b.physical_size()).sum()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if offset >= self.capacity || buf.is_empty() {
            return Ok(0);
        }

        let max_readable = (self.capacity - offset) as usize;
        let to_read = buf.len().min(max_readable);
        let target_buf = &mut buf[..to_read];

        let block_sz = self.block_size as u64;
        let mut cur_offset = offset;
        let mut buf_pos = 0;

        let blocks_guard = self.blocks.read();

        while buf_pos < to_read {
            let block_idx = cur_offset / block_sz;
            let in_block_offset = (cur_offset % block_sz) as usize;
            let bytes_in_this_block = (self.block_size - in_block_offset).min(to_read - buf_pos);

            let dest_slice = &mut target_buf[buf_pos..buf_pos + bytes_in_this_block];

            if let Some(block) = blocks_guard.get(&block_idx) {
                let data = block.get_data()?;
                dest_slice.copy_from_slice(&data[in_block_offset..in_block_offset + bytes_in_this_block]);
            } else {
                // Sparse block: fill zeros
                dest_slice.fill(0);
            }

            cur_offset += bytes_in_this_block as u64;
            buf_pos += bytes_in_this_block;
        }

        self.stats.record_read(to_read);
        Ok(to_read)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> Result<usize, StorageError> {
        if buf.is_empty() {
            return Ok(0);
        }

        if offset + buf.len() as u64 > self.capacity {
            return Err(StorageError::OutOfBounds {
                offset: offset + buf.len() as u64,
                capacity: self.capacity,
            });
        }

        let block_sz = self.block_size as u64;
        let mut cur_offset = offset;
        let mut buf_pos = 0;
        let to_write = buf.len();

        let mut blocks_guard = self.blocks.write();

        while buf_pos < to_write {
            let block_idx = cur_offset / block_sz;
            let in_block_offset = (cur_offset % block_sz) as usize;
            let bytes_in_this_block = (self.block_size - in_block_offset).min(to_write - buf_pos);
            let src_slice = &buf[buf_pos..buf_pos + bytes_in_this_block];

            if in_block_offset == 0 && bytes_in_this_block == self.block_size {
                // Fast path: writing full block completely overwrites
                let block = Block::new(src_slice, self.compression);
                blocks_guard.insert(block_idx, block);
            } else {
                // Partial block write: read existing or create zero-block, update, re-encode
                let mut block_data = if let Some(existing) = blocks_guard.get(&block_idx) {
                    existing.get_data()?.into_owned()
                } else {
                    vec![0u8; self.block_size]
                };

                block_data[in_block_offset..in_block_offset + bytes_in_this_block]
                    .copy_from_slice(src_slice);

                let block = Block::new(&block_data, self.compression);
                blocks_guard.insert(block_idx, block);
            }

            cur_offset += bytes_in_this_block as u64;
            buf_pos += bytes_in_this_block;
        }

        self.stats.record_write(to_write);
        Ok(to_write)
    }

    fn flush(&self) -> Result<(), StorageError> {
        // In-memory flush is a no-op
        Ok(())
    }
}
