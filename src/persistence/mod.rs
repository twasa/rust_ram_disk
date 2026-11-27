use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use thiserror::Error;

use crate::engine::sparse_storage::{SparseMemoryStorage, StorageEngine};
use crate::engine::CompressionMode;

const MAGIC_SPARSE_HEADER: &[u8; 8] = b"RAMDSK01";

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("I/O error during image operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid or corrupted RAM disk image header")]
    InvalidHeader,
    #[error("Storage engine error: {0}")]
    Storage(#[from] crate::engine::sparse_storage::StorageError),
}

pub struct ImagePersistence;

impl ImagePersistence {
    /// Saves a sparse snapshot of the RAM disk to an image file.
    /// Only allocated blocks are saved to disk, preserving storage space.
    pub fn save_sparse(storage: &SparseMemoryStorage, path: &Path) -> Result<(), PersistenceError> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Header: Magic(8) + Capacity(8) + BlockSize(4) + BlockCount(8)
        let allocated_indices = storage.get_allocated_block_indices();
        writer.write_all(MAGIC_SPARSE_HEADER)?;
        writer.write_all(&storage.capacity().to_le_bytes())?;
        writer.write_all(&(storage.block_size() as u32).to_le_bytes())?;
        writer.write_all(&(allocated_indices.len() as u64).to_le_bytes())?;

        let mut buf = vec![0u8; storage.block_size()];
        for idx in allocated_indices {
            let offset = idx * storage.block_size() as u64;
            let read_bytes = storage.read_at(offset, &mut buf)?;
            if read_bytes > 0 {
                writer.write_all(&idx.to_le_bytes())?;
                writer.write_all(&(read_bytes as u32).to_le_bytes())?;
                writer.write_all(&buf[..read_bytes])?;
            }
        }

        writer.flush()?;
        Ok(())
    }

    /// Loads a sparse snapshot from an image file into a new SparseMemoryStorage.
    pub fn load_sparse(
        path: &Path,
        compression: CompressionMode,
    ) -> Result<SparseMemoryStorage, PersistenceError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC_SPARSE_HEADER {
            return Err(PersistenceError::InvalidHeader);
        }

        let mut cap_bytes = [0u8; 8];
        reader.read_exact(&mut cap_bytes)?;
        let capacity = u64::from_le_bytes(cap_bytes);

        let mut blk_sz_bytes = [0u8; 4];
        reader.read_exact(&mut blk_sz_bytes)?;
        let block_size = u32::from_le_bytes(blk_sz_bytes) as usize;

        let mut blk_cnt_bytes = [0u8; 8];
        reader.read_exact(&mut blk_cnt_bytes)?;
        let block_count = u64::from_le_bytes(blk_cnt_bytes);

        let storage = SparseMemoryStorage::new(capacity, block_size, compression);

        let mut data_buf = vec![0u8; block_size];
        for _ in 0..block_count {
            let mut idx_bytes = [0u8; 8];
            reader.read_exact(&mut idx_bytes)?;
            let idx = u64::from_le_bytes(idx_bytes);

            let mut len_bytes = [0u8; 4];
            reader.read_exact(&mut len_bytes)?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            if len > data_buf.len() {
                data_buf.resize(len, 0);
            }
            reader.read_exact(&mut data_buf[..len])?;

            let offset = idx * block_size as u64;
            storage.write_at(offset, &data_buf[..len])?;
        }

        Ok(storage)
    }

    /// Exports the RAM disk as a flat raw disk image (e.g. for mounting with OS tools or 7-zip).
    pub fn export_raw_flat(storage: &SparseMemoryStorage, path: &Path) -> Result<(), PersistenceError> {
        let mut file = File::create(path)?;
        file.set_len(storage.capacity())?;

        let block_size = storage.block_size();
        let mut buf = vec![0u8; block_size];

        for idx in storage.get_allocated_block_indices() {
            let offset = idx * block_size as u64;
            let n = storage.read_at(offset, &mut buf)?;
            if n > 0 {
                file.seek(SeekFrom::Start(offset))?;
                file.write_all(&buf[..n])?;
            }
        }

        file.flush()?;
        Ok(())
    }
}
