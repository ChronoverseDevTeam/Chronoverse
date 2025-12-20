mod bundle;
mod chunk;
mod constants;
mod error;
mod index;
mod io_utils;
mod layout;

pub use bundle::{PackBundle, PackIdentity};
pub use chunk::{
    ChunkHash, ChunkRecord, Compression, EncodedChunk, KNOWN_FLAG_MASK, compute_chunk_hash,
};
pub use constants::*;
pub use error::{RepositoryError, Result};
pub use index::{IndexEntry, IndexSnapshot, MutableIndex};
pub use io_utils::{
    blake3_hash_to_hex, blake3_hex_to_hash, compute_blake3_bytes, compute_blake3_str, Blake3Stream,
};
pub use layout::{Repository, RepositoryLayout};
