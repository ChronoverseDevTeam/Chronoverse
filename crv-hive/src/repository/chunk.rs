use std::borrow::Cow;

use blake3::Hasher;
use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};

use super::constants::{HASH_SIZE, PACK_ENTRY_FIXED_SECTION};
use super::error::{RepositoryError, Result};

pub type ChunkHash = [u8; HASH_SIZE];

pub const LZ4_FLAG: u16 = 0x0001;
pub const KNOWN_FLAG_MASK: u16 = LZ4_FLAG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Lz4,
}

impl Compression {
    pub fn to_flags(self) -> u16 {
        match self {
            Compression::None => 0,
            Compression::Lz4 => LZ4_FLAG,
        }
    }

    pub fn from_flags(flags: u16) -> Result<Self> {
        if flags & !KNOWN_FLAG_MASK != 0 {
            return Err(RepositoryError::UnsupportedCompression(flags));
        }
        if flags & LZ4_FLAG != 0 {
            Ok(Compression::Lz4)
        } else {
            Ok(Compression::None)
        }
    }

    pub fn encode<'a>(self, original: &'a [u8]) -> Result<EncodedChunk<'a>> {
        match self {
            Compression::None => Ok(EncodedChunk {
                payload: Cow::Borrowed(original),
                compression: Compression::None,
            }),
            Compression::Lz4 => Ok(EncodedChunk {
                payload: Cow::Owned(compress_prepend_size(original)),
                compression: Compression::Lz4,
            }),
        }
    }

    pub fn decode(&self, encoded: &[u8]) -> Result<Vec<u8>> {
        match self {
            Compression::None => Ok(encoded.to_vec()),
            Compression::Lz4 => decompress_size_prepended(encoded)
                .map_err(|_| RepositoryError::Corrupted("LZ4 数据损坏")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EncodedChunk<'a> {
    pub payload: Cow<'a, [u8]>,
    pub compression: Compression,
}

impl<'a> EncodedChunk<'a> {
    pub fn len(&self) -> usize {
        self.payload.len()
    }
}

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub hash: ChunkHash,
    pub offset: u64,
    pub stored_len: u32,
    pub logical_len: u32,
    pub flags: u16,
}

impl ChunkRecord {
    pub fn compression(&self) -> Result<Compression> {
        Compression::from_flags(self.flags)
    }

    pub fn entry_bytes(&self) -> u64 {
        PACK_ENTRY_FIXED_SECTION + self.stored_len as u64
    }
}

pub fn compute_chunk_hash(data: &[u8]) -> ChunkHash {
    let mut hasher = Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}
