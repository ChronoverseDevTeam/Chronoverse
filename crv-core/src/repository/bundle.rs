use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::chunk::{ChunkHash, ChunkRecord, Compression, compute_chunk_hash};
use super::error::{RepositoryError, Result};
use super::index::{IndexEntry, MutableIndex};
use super::layout::RepositoryLayout;
use super::constants::{
    PACK_ENTRY_FIXED_SECTION, PACK_HEADER_SIZE, PACK_MAGIC, PACK_TRAILER_SIZE, PACK_VERSION,
};
use super::io_utils::{compute_crc32, ensure_parent_dir};

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

#[derive(Debug, Default, Clone)]
pub struct PackStats {
    pub chunk_count: u64,
    pub logical_bytes: u64,
    pub physical_bytes: u64,
}

impl PackStats {
    fn apply_chunk(&mut self, logical_len: u32, stored_len: u32) {
        self.chunk_count += 1;
        self.logical_bytes += logical_len as u64;
        self.physical_bytes += PACK_ENTRY_FIXED_SECTION + stored_len as u64;
    }

    fn rollback_chunk(&mut self, logical_len: u32, stored_len: u32) {
        self.chunk_count = self.chunk_count.saturating_sub(1);
        self.logical_bytes = self.logical_bytes.saturating_sub(logical_len as u64);
        self.physical_bytes = self
            .physical_bytes
            .saturating_sub(PACK_ENTRY_FIXED_SECTION + stored_len as u64);
    }
}

pub(crate) struct PackWriter {
    file: File,
    path: PathBuf,
    sealed: bool,
    stats: PackStats,
}

impl PackWriter {
    pub fn create_new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        ensure_parent_dir(path)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)?;
        write_pack_header(&mut file)?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            sealed: false,
            stats: PackStats::default(),
        })
    }

    fn ensure_open(&self) -> Result<()> {
        if self.sealed {
            Err(RepositoryError::AlreadySealed {
                path: self.path.clone(),
            })
        } else {
            Ok(())
        }
    }

    pub fn append_chunk(
        &mut self,
        hash: ChunkHash,
        logical_len: u32,
        flags: u16,
        payload: &[u8],
    ) -> Result<ChunkRecord> {
        self.ensure_open()?;
        let stored_len = u32::try_from(payload.len())
            .map_err(|_| RepositoryError::ChunkTooLarge(payload.len()))?;
        let offset = self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&stored_len.to_le_bytes())?;
        self.file.write_all(&flags.to_le_bytes())?;
        self.file.write_all(&hash)?;
        self.file.write_all(payload)?;
        self.file.flush()?;
        self.stats.apply_chunk(logical_len, stored_len);
        Ok(ChunkRecord {
            hash,
            offset,
            stored_len,
            logical_len,
            flags,
        })
    }

    pub fn rewind(&mut self, record: &ChunkRecord) -> Result<()> {
        self.ensure_open()?;
        self.file.set_len(record.offset)?;
        self.file.seek(SeekFrom::End(0))?;
        self.stats
            .rollback_chunk(record.logical_len, record.stored_len);
        Ok(())
    }

    pub fn seal(&mut self) -> Result<()> {
        self.ensure_open()?;
        self.file.flush()?;
        let data_len = self.file.metadata()?.len();
        let crc = compute_crc32(&self.file, data_len)?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.flush()?;
        self.file.sync_all()?;
        self.sealed = true;
        Ok(())
    }

    pub fn stats(&self) -> &PackStats {
        &self.stats
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) struct PackReader {
    file: File,
    data_len: u64,
}

impl PackReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = OpenOptions::new().read(true).open(path)?;
        let total_len = file.metadata()?.len();
        if total_len < PACK_HEADER_SIZE {
            return Err(RepositoryError::Corrupted("pack 文件长度非法"));
        }
        verify_pack_header(&mut file)?;
        let (data_len, _sealed) = detect_data_len(&mut file, total_len)?;
        Ok(Self {
            file,
            data_len,
        })
    }

    pub fn read_chunk(&mut self, entry: &IndexEntry) -> Result<Vec<u8>> {
        let end = entry.offset + PACK_ENTRY_FIXED_SECTION + entry.length as u64;
        if end > self.data_len {
            return Err(RepositoryError::Corrupted("索引 offset 超出 pack 长度"));
        }
        self.file.seek(SeekFrom::Start(entry.offset))?;
        let stored_len = read_u32(&mut self.file)?;
        if stored_len != entry.length {
            return Err(RepositoryError::Corrupted("索引长度与 pack 不匹配"));
        }
        let flags = read_u16(&mut self.file)?;
        if flags != entry.flags {
            return Err(RepositoryError::Corrupted("索引 flags 与 pack 不匹配"));
        }
        let mut hash = [0u8; super::constants::HASH_SIZE];
        self.file.read_exact(&mut hash)?;
        if hash != entry.hash {
            return Err(RepositoryError::Corrupted("索引 hash 与 pack 不匹配"));
        }
        let mut payload = vec![0u8; stored_len as usize];
        self.file.read_exact(&mut payload)?;
        let compression = Compression::from_flags(flags)?;
        compression.decode(&payload)
    }
}

fn write_pack_header(file: &mut File) -> Result<()> {
    file.write_all(&PACK_MAGIC.to_le_bytes())?;
    file.write_all(&PACK_VERSION.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    Ok(())
}

fn verify_pack_header(file: &mut File) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let magic = read_u32(file)?;
    if magic != PACK_MAGIC {
        return Err(RepositoryError::InvalidMagic {
            expected: PACK_MAGIC,
            actual: magic,
        });
    }
    let version = read_u16(file)?;
    if version != PACK_VERSION {
        return Err(RepositoryError::InvalidVersion {
            expected: PACK_VERSION,
            actual: version,
        });
    }
    let reserved = read_u32(file)?;
    if reserved != 0 {
        return Err(RepositoryError::ReservedNonZero);
    }
    Ok(())
}

fn detect_data_len(file: &mut File, total_len: u64) -> Result<(u64, bool)> {
    if total_len < PACK_HEADER_SIZE + PACK_TRAILER_SIZE {
        return Ok((total_len, false));
    }
    let data_len = total_len - PACK_TRAILER_SIZE;
    file.seek(SeekFrom::Start(data_len))?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf)?;
    let stored_crc = u32::from_le_bytes(buf);
    let crc = compute_crc32(file, data_len)?;
    if crc == stored_crc {
        Ok((data_len, true))
    } else {
        Ok((total_len, false))
    }
}

fn read_u32(file: &mut File) -> Result<u32> {
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u16(file: &mut File) -> Result<u16> {
    let mut buf = [0u8; 2];
    file.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{IndexSnapshot, RepositoryLayout};

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
