use std::cmp::Ordering;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::chunk::ChunkHash;
use super::constants::{
    HASH_SIZE, INDEX_ENTRY_SIZE, INDEX_HEADER_SIZE, INDEX_MAGIC, INDEX_TRAILER_SIZE, INDEX_VERSION,
};
use super::error::{RepositoryError, Result};
use super::io_utils::{compute_crc32, ensure_parent_dir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub hash: ChunkHash,
    pub offset: u64,
    pub length: u32,
    pub flags: u16,
}

impl IndexEntry {
    pub fn new(hash: ChunkHash, offset: u64, length: u32, flags: u16) -> Self {
        Self {
            hash,
            offset,
            length,
            flags,
        }
    }
}

pub struct MutableIndex {
    path: PathBuf,
    file: File,
    entries: Vec<IndexEntry>,
    sealed: bool,
}

impl MutableIndex {
    pub fn create_new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        ensure_parent_dir(path)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)?;
        write_header(&mut file, 0)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            entries: Vec::new(),
            sealed: false,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let (entries, sealed) = load_entries(&mut file, path)?;
        if sealed {
            return Err(RepositoryError::AlreadySealed {
                path: path.to_path_buf(),
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            entries,
            sealed: false,
        })
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub fn find(&self, hash: &ChunkHash) -> Option<&IndexEntry> {
        self.entries
            .binary_search_by(|entry| entry.hash.cmp(hash))
            .ok()
            .and_then(|idx| self.entries.get(idx))
    }

    pub fn contains(&self, hash: &ChunkHash) -> bool {
        self.find(hash).is_some()
    }

    pub fn insert(&mut self, entry: IndexEntry) -> Result<()> {
        self.ensure_open()?;
        match self.entries.binary_search_by(|e| e.hash.cmp(&entry.hash)) {
            Ok(_) => {
                return Err(RepositoryError::DuplicateHash { hash: entry.hash });
            }
            Err(pos) => {
                self.entries.insert(pos, entry);
            }
        }
        self.persist()
    }

    pub fn seal(&mut self) -> Result<()> {
        self.ensure_open()?;
        self.persist()?;
        let len = self.file.metadata()?.len();
        let crc = compute_crc32(&self.file, len)?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&crc.to_le_bytes())?;
        self.file.flush()?;
        self.file.sync_all()?;
        self.sealed = true;
        Ok(())
    }

    fn persist(&mut self) -> Result<()> {
        self.ensure_open()?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.set_len(0)?;
        let entry_count =
            u64::try_from(self.entries.len()).map_err(|_| RepositoryError::EntryCountOverflow)?;
        write_header(&mut self.file, entry_count)?;
        for entry in &self.entries {
            write_entry(&mut self.file, entry)?;
        }
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
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
}

pub struct IndexSnapshot {
    entries: Vec<IndexEntry>,
    sealed: bool,
}

impl IndexSnapshot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = OpenOptions::new().read(true).open(path)?;
        let (entries, sealed) = load_entries(&mut file, path)?;
        Ok(Self { entries, sealed })
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub fn find(&self, hash: &ChunkHash) -> Option<&IndexEntry> {
        self.entries
            .binary_search_by(|entry| entry.hash.cmp(hash))
            .ok()
            .and_then(|idx| self.entries.get(idx))
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }
}

fn write_header(file: &mut File, entry_count: u64) -> Result<()> {
    file.write_all(&INDEX_MAGIC.to_le_bytes())?;
    file.write_all(&INDEX_VERSION.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&entry_count.to_le_bytes())?;
    Ok(())
}

fn write_entry(file: &mut File, entry: &IndexEntry) -> Result<()> {
    file.write_all(&entry.hash)?;
    file.write_all(&entry.offset.to_le_bytes())?;
    file.write_all(&entry.length.to_le_bytes())?;
    file.write_all(&entry.flags.to_le_bytes())?;
    Ok(())
}

fn load_entries(file: &mut File, path: &Path) -> Result<(Vec<IndexEntry>, bool)> {
    file.seek(SeekFrom::Start(0))?;
    let magic = read_u32(file)?;
    if magic != INDEX_MAGIC {
        return Err(RepositoryError::InvalidMagic {
            expected: INDEX_MAGIC,
            actual: magic,
        });
    }
    let version = read_u16(file)?;
    if version != INDEX_VERSION {
        return Err(RepositoryError::InvalidVersion {
            expected: INDEX_VERSION,
            actual: version,
        });
    }
    let reserved = read_u32(file)?;
    if reserved != 0 {
        return Err(RepositoryError::ReservedNonZero);
    }
    let entry_count = read_u64(file)?;
    let entry_count_usize =
        usize::try_from(entry_count).map_err(|_| RepositoryError::EntryCountOverflow)?;
    let entries_len = INDEX_ENTRY_SIZE
        .checked_mul(entry_count)
        .ok_or(RepositoryError::EntryCountOverflow)?;
    let expected_data_len = INDEX_HEADER_SIZE
        .checked_add(entries_len)
        .ok_or(RepositoryError::EntryCountOverflow)?;
    let total_len = file.metadata()?.len();
    let sealed = match total_len.cmp(&(expected_data_len + INDEX_TRAILER_SIZE)) {
        Ordering::Equal => {
            let data_len = expected_data_len;
            file.seek(SeekFrom::Start(data_len))?;
            let mut crc_buf = [0u8; 4];
            file.read_exact(&mut crc_buf)?;
            let stored_crc = u32::from_le_bytes(crc_buf);
            let calculated = compute_crc32(file, data_len)?;
            if stored_crc != calculated {
                return Err(RepositoryError::CrcMismatch {
                    path: path.to_path_buf(),
                });
            }
            true
        }
        Ordering::Greater => {
            return Err(RepositoryError::Corrupted(
                "idx 文件长度与 entry_count 不匹配",
            ));
        }
        Ordering::Less => false,
    };

    if !sealed && total_len != expected_data_len {
        return Err(RepositoryError::Corrupted("未封存 idx 文件长度非法"));
    }

    file.seek(SeekFrom::Start(INDEX_HEADER_SIZE))?;
    let mut entries = Vec::with_capacity(entry_count_usize);
    let mut prev_hash: Option<ChunkHash> = None;
    for _ in 0..entry_count_usize {
        let mut hash = [0u8; HASH_SIZE];
        file.read_exact(&mut hash)?;
        let offset = read_u64(file)?;
        let length = read_u32(file)?;
        let flags = read_u16(file)?;
        if let Some(prev) = prev_hash {
            if prev >= hash {
                return Err(RepositoryError::IndexOutOfOrder);
            }
        }
        prev_hash = Some(hash);
        entries.push(IndexEntry {
            hash,
            offset,
            length,
            flags,
        });
    }

    Ok((entries, sealed))
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

fn read_u64(file: &mut File) -> Result<u64> {
    let mut buf = [0u8; 8];
    file.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}
