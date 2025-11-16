use std::path::PathBuf;

use super::chunk::{ChunkHash, ChunkRecord, Compression, compute_chunk_hash};
use super::error::{RepositoryError, Result};
use super::index::{IndexEntry, MutableIndex};
use super::layout::RepositoryLayout;
use super::pack::{PackStats, PackWriter};

pub struct PackIdentity {
    pub shard: u8,
    pub pack_id: u32,
    pub base_name: String,
    pub directory: PathBuf,
}

pub struct PackBundle {
    identity: PackIdentity,
    pack: PackWriter,
    index: MutableIndex,
}

impl PackBundle {
    pub fn create(layout: &RepositoryLayout, shard: u8, pack_id: u32) -> Result<Self> {
        let (dat_path, idx_path) = layout.pack_paths(shard, pack_id)?;
        let identity = PackIdentity {
            shard,
            pack_id,
            base_name: RepositoryLayout::pack_base_name(pack_id),
            directory: dat_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| layout.root().to_path_buf()),
        };
        Ok(Self {
            identity,
            pack: PackWriter::create_new(dat_path)?,
            index: MutableIndex::create_new(idx_path)?,
        })
    }

    pub fn append_chunk(&mut self, data: &[u8], compression: Compression) -> Result<ChunkRecord> {
        let hash = compute_chunk_hash(data);
        if self.index.contains(&hash) {
            return Err(RepositoryError::DuplicateHash { hash });
        }
        let logical_len =
            u32::try_from(data.len()).map_err(|_| RepositoryError::ChunkTooLarge(data.len()))?;
        let encoded = compression.encode(data)?;
        let record = self.pack.append_chunk(
            hash,
            logical_len,
            encoded.compression.to_flags(),
            encoded.payload.as_ref(),
        )?;
        let entry = IndexEntry::new(record.hash, record.offset, record.stored_len, record.flags);
        if let Err(err) = self.index.insert(entry) {
            self.pack.rewind(&record)?;
            return Err(err);
        }
        Ok(record)
    }

    pub fn seal(&mut self) -> Result<()> {
        self.index.seal()?;
        self.pack.seal()
    }

    pub fn stats(&self) -> &PackStats {
        self.pack.stats()
    }

    pub fn identity(&self) -> &PackIdentity {
        &self.identity
    }

    pub fn find_entry(&self, hash: &ChunkHash) -> Option<IndexEntry> {
        self.index.find(hash).cloned()
    }

    pub fn pack_path(&self) -> PathBuf {
        self.pack.path().to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{IndexSnapshot, PackReader, RepositoryLayout};

    #[test]
    fn pack_bundle_roundtrip() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let layout = RepositoryLayout::new(temp_dir.path());
        let mut bundle = PackBundle::create(&layout, 0xAA, 1)?;

        let chunk_a = bundle.append_chunk(b"hello world", Compression::None)?;
        let chunk_b = bundle.append_chunk(b"crv repository data", Compression::Lz4)?;

        bundle.seal()?;

        let (dat_path, idx_path) = layout.pack_paths(0xAA, 1)?;

        let snapshot = IndexSnapshot::open(&idx_path)?;
        assert_eq!(snapshot.entries().len(), 2);

        let entry_a = snapshot.find(&chunk_a.hash).expect("chunk a entry");
        let entry_b = snapshot.find(&chunk_b.hash).expect("chunk b entry");

        let mut reader = PackReader::open(&dat_path)?;
        assert_eq!(reader.read_chunk(entry_a)?, b"hello world");
        assert_eq!(reader.read_chunk(entry_b)?, b"crv repository data".to_vec());

        Ok(())
    }
}
