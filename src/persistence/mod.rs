use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::engine::sparse_storage::{SparseMemoryStorage, StorageEngine};
use crate::engine::CompressionMode;

const MAGIC_SPARSE_HEADER: &[u8; 8] = b"RAMDSK01";
const MAGIC_DRIVE_SNAPSHOT: &[u8; 8] = b"RAMSNAP1";

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

    /// 將已掛載磁碟機（例如 "S:"）內的所有檔案結構壓縮打包為快照檔
    pub fn save_drive_snapshot(
        mount_point: &str,
        snapshot_path: &Path,
        compress: CompressionMode,
    ) -> Result<usize, PersistenceError> {
        let drive_root = format!("{}\\", mount_point.trim_end_matches('\\'));
        let root_path = Path::new(&drive_root);

        let file = File::create(snapshot_path)?;
        let mut writer = BufWriter::new(file);

        writer.write_all(MAGIC_DRIVE_SNAPSHOT)?;
        writer.write_all(&[if compress == CompressionMode::Lz4 { 1 } else { 0 }])?;

        let mut entries = Vec::new();
        if root_path.exists() {
            Self::collect_entries(root_path, root_path, &mut entries)?;
        }

        writer.write_all(&(entries.len() as u32).to_le_bytes())?;

        let mut saved_count = 0;
        for rel_path in entries {
            let full_path = root_path.join(&rel_path);
            let metadata = std::fs::symlink_metadata(&full_path)?;
            if metadata.is_file() {
                let data = std::fs::read(&full_path)?;
                let rel_str = rel_path.to_string_lossy();
                let rel_bytes = rel_str.as_bytes();

                writer.write_all(&(rel_bytes.len() as u32).to_le_bytes())?;
                writer.write_all(rel_bytes)?;

                if compress == CompressionMode::Lz4 {
                    let compressed = lz4_flex::compress_prepend_size(&data);
                    writer.write_all(&(compressed.len() as u64).to_le_bytes())?;
                    writer.write_all(&compressed)?;
                } else {
                    writer.write_all(&(data.len() as u64).to_le_bytes())?;
                    writer.write_all(&data)?;
                }
                saved_count += 1;
            }
        }

        writer.flush()?;
        Ok(saved_count)
    }

    /// 從快照檔將檔案結構解壓縮還原至目標磁碟機（例如 "S:"）
    pub fn restore_drive_snapshot(
        snapshot_path: &Path,
        mount_point: &str,
    ) -> Result<usize, PersistenceError> {
        let drive_root = format!("{}\\", mount_point.trim_end_matches('\\'));
        let root_path = Path::new(&drive_root);

        let file = File::open(snapshot_path)?;
        let mut reader = BufReader::new(file);

        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC_DRIVE_SNAPSHOT {
            return Err(PersistenceError::InvalidHeader);
        }

        let mut comp_byte = [0u8; 1];
        reader.read_exact(&mut comp_byte)?;
        let is_lz4 = comp_byte[0] == 1;

        let mut count_bytes = [0u8; 4];
        reader.read_exact(&mut count_bytes)?;
        let entry_count = u32::from_le_bytes(count_bytes);

        let mut restored_count = 0;
        for _ in 0..entry_count {
            let mut name_len_bytes = [0u8; 4];
            reader.read_exact(&mut name_len_bytes)?;
            let name_len = u32::from_le_bytes(name_len_bytes) as usize;

            let mut name_bytes = vec![0u8; name_len];
            reader.read_exact(&mut name_bytes)?;
            let rel_str = String::from_utf8_lossy(&name_bytes).to_string();

            let mut data_len_bytes = [0u8; 8];
            reader.read_exact(&mut data_len_bytes)?;
            let data_len = u64::from_le_bytes(data_len_bytes) as usize;

            let mut payload = vec![0u8; data_len];
            reader.read_exact(&mut payload)?;

            let raw_data = if is_lz4 {
                lz4_flex::decompress_size_prepended(&payload)
                    .map_err(|e| PersistenceError::Storage(crate::engine::sparse_storage::StorageError::BlockError(crate::engine::block::BlockError::DecompressionError(e.to_string()))))?
            } else {
                payload
            };

            let dest_file = root_path.join(rel_str);
            if let Some(parent) = dest_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest_file, &raw_data)?;
            restored_count += 1;
        }

        Ok(restored_count)
    }

    fn collect_entries(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                Self::collect_entries(base, &path, out)?;
            } else if path.is_file() {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_path_buf());
                }
            }
        }
        Ok(())
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
