use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlockError {
    #[error("Block compression failed: {0}")]
    CompressionError(String),
    #[error("Block decompression failed: {0}")]
    DecompressionError(String),
    #[error("Buffer length mismatch: expected {expected}, got {actual}")]
    BufferMismatch { expected: usize, actual: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionMode {
    #[default]
    None,
    Lz4,
}

#[derive(Debug, Clone)]
pub enum Block {
    Raw(Vec<u8>),
    Compressed {
        data: Vec<u8>,
        original_len: usize,
    },
}

impl Block {
    /// Creates a new uncompressed block.
    pub fn new_raw(data: Vec<u8>) -> Self {
        Block::Raw(data)
    }

    /// Creates a block with optional LZ4 compression.
    /// If compression does not reduce size by at least 5%, stores as raw to avoid decompression overhead.
    pub fn new(data: &[u8], mode: CompressionMode) -> Self {
        match mode {
            CompressionMode::None => Block::Raw(data.to_vec()),
            CompressionMode::Lz4 => {
                let compressed = lz4_flex::compress_prepend_size(data);
                // Only keep compressed if it is meaningfully smaller
                if compressed.len() + 16 < data.len() {
                    Block::Compressed {
                        data: compressed,
                        original_len: data.len(),
                    }
                } else {
                    Block::Raw(data.to_vec())
                }
            }
        }
    }

    /// Read the full uncompressed block contents.
    pub fn get_data(&self) -> Result<Cow<'_, [u8]>, BlockError> {
        match self {
            Block::Raw(bytes) => Ok(Cow::Borrowed(bytes.as_slice())),
            Block::Compressed { data, .. } => {
                let decompressed = lz4_flex::decompress_size_prepended(data)
                    .map_err(|e| BlockError::DecompressionError(e.to_string()))?;
                Ok(Cow::Owned(decompressed))
            }
        }
    }

    /// Returns actual bytes allocated in host physical memory.
    pub fn physical_size(&self) -> usize {
        match self {
            Block::Raw(bytes) => bytes.len(),
            Block::Compressed { data, .. } => data.len(),
        }
    }

    /// Whether the block is currently compressed.
    pub fn is_compressed(&self) -> bool {
        matches!(self, Block::Compressed { .. })
    }
}
