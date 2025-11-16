use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use super::bundle::PackBundle;
use super::chunk::{ChunkHash, ChunkRecord, Compression, compute_chunk_hash};
use super::constants::{PACK_DATA_SUFFIX, PACK_FILE_PREFIX};
use super::error::{RepositoryError, Result};
use super::index::{IndexEntry, IndexSnapshot};
use super::layout::RepositoryLayout;
use super::pack::PackReader;

const DEFAULT_PACK_SOFT_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_HARD_PACK_SIZE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_HARD_PACK_CHUNK_LIMIT: u64 = 100_000;

pub struct RepositoryManager {
    layout: RepositoryLayout,
    shards: Vec<RwLock<ShardState>>,
    pack_soft_limit: u64,
    hard_size_limit: u64,
    hard_chunk_limit: u64,
}

impl RepositoryManager {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        Self::with_pack_soft_limit(root, DEFAULT_PACK_SOFT_LIMIT_BYTES)
    }

    pub fn with_pack_soft_limit(root: impl Into<PathBuf>, limit: u64) -> Result<Self> {
        Self::with_limits(
            root,
            limit,
            DEFAULT_HARD_PACK_SIZE_LIMIT_BYTES,
            DEFAULT_HARD_PACK_CHUNK_LIMIT,
        )
    }

    pub fn with_limits(
        root: impl Into<PathBuf>,
        pack_soft_limit: u64,
        hard_size_limit: u64,
        hard_chunk_limit: u64,
    ) -> Result<Self> {
        let layout = RepositoryLayout::new(root);
        let mut shards = Vec::with_capacity(256);
        for shard in 0u16..=0xFF {
            let shard = shard as u8;
            let (known_packs, next_pack_id) = discover_existing_packs(&layout, shard)?;
            shards.push(RwLock::new(ShardState::new(known_packs, next_pack_id)));
        }
        Ok(Self {
            layout,
            shards,
            pack_soft_limit: pack_soft_limit.max(1),
            hard_size_limit: hard_size_limit.max(1),
            hard_chunk_limit: hard_chunk_limit.max(1),
        })
    }

    pub fn write_chunk(&self, data: &[u8], compression: Compression) -> Result<ChunkRecord> {
        let hash = compute_chunk_hash(data);
        let shard = hash[0];
        let lock = &self.shards[shard as usize];
        let mut guard = lock.write().expect("shard lock poisoned");
        guard.enforce_hard_limits(self.hard_size_limit, self.hard_chunk_limit)?;
        if guard.active_contains(&hash) {
            return Err(RepositoryError::DuplicateHash { hash });
        }
        let sealed_pack_ids = guard.sealed_pack_ids();
        if locate_in_pack_ids(&self.layout, shard, &sealed_pack_ids, &hash)?.is_some() {
            return Err(RepositoryError::DuplicateHash { hash });
        }
        let bundle = guard.ensure_active_bundle(&self.layout, shard)?;
        let record = bundle.append_chunk(data, compression)?;
        if bundle.stats().physical_bytes >= self.pack_soft_limit {
            let _ = guard.seal_active()?;
        }
        Ok(record)
    }

    pub fn read_chunk(&self, hash: &ChunkHash) -> Result<Vec<u8>> {
        if let Some((entry, dat_path)) = self.locate_chunk(hash)? {
            let mut reader = PackReader::open(&dat_path)?;
            return reader.read_chunk(&entry);
        }
        Err(RepositoryError::ChunkNotFound { hash: *hash })
    }

    pub fn seal_shard(&self, shard: u8) -> Result<()> {
        let lock = &self.shards[shard as usize];
        let mut guard = lock.write().expect("shard lock poisoned");
        let _ = guard.seal_active()?;
        Ok(())
    }

    pub fn seal_all(&self) -> Result<()> {
        for shard in 0u16..=0xFF {
            self.seal_shard(shard as u8)?;
        }
        Ok(())
    }

    pub fn seal_bundle(&self, shard: u8, pack_id: u32) -> Result<bool> {
        let lock = &self.shards[shard as usize];
        let mut guard = lock.write().expect("shard lock poisoned");
        guard.seal_specific(pack_id)
    }

    pub fn locate_chunk(&self, hash: &ChunkHash) -> Result<Option<(IndexEntry, PathBuf)>> {
        let shard = hash[0];
        let lock = &self.shards[shard as usize];
        let sealed_pack_ids = {
            let guard = lock.read().expect("shard lock poisoned");
            if let Some(result) = guard.find_in_active(hash) {
                return Ok(Some(result));
            }
            guard.sealed_pack_ids()
        };
        locate_in_pack_ids(&self.layout, shard, &sealed_pack_ids, hash)
    }
}

struct ShardState {
    known_packs: BTreeSet<u32>,
    next_pack_id: u32,
    active: Option<PackBundle>,
}

impl ShardState {
    fn new(known_packs: BTreeSet<u32>, next_pack_id: u32) -> Self {
        Self {
            known_packs,
            next_pack_id,
            active: None,
        }
    }

    fn sealed_pack_ids(&self) -> Vec<u32> {
        let active_id = self.active.as_ref().map(|bundle| bundle.identity().pack_id);
        self.known_packs
            .iter()
            .copied()
            .filter(|id| Some(*id) != active_id)
            .collect()
    }

    fn active_contains(&self, hash: &ChunkHash) -> bool {
        self.active
            .as_ref()
            .and_then(|bundle| bundle.find_entry(hash))
            .is_some()
    }

    fn find_in_active(&self, hash: &ChunkHash) -> Option<(IndexEntry, PathBuf)> {
        self.active.as_ref().and_then(|bundle| {
            bundle
                .find_entry(hash)
                .map(|entry| (entry, bundle.pack_path()))
        })
    }

    fn ensure_active_bundle(
        &mut self,
        layout: &RepositoryLayout,
        shard: u8,
    ) -> Result<&mut PackBundle> {
        if self.active.is_none() {
            let pack_id = self.next_pack_id;
            let next = pack_id
                .checked_add(1)
                .ok_or(RepositoryError::PackIdOverflow)?;
            let bundle = PackBundle::create(layout, shard, pack_id)?;
            self.known_packs.insert(pack_id);
            self.active = Some(bundle);
            self.next_pack_id = next;
        }
        Ok(self.active.as_mut().expect("active bundle must exist"))
    }

    fn seal_active(&mut self) -> Result<bool> {
        if let Some(mut bundle) = self.active.take() {
            bundle.seal()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn seal_specific(&mut self, pack_id: u32) -> Result<bool> {
        if self
            .active
            .as_ref()
            .map(|bundle| bundle.identity().pack_id == pack_id)
            .unwrap_or(false)
        {
            self.seal_active()
        } else {
            Ok(false)
        }
    }

    fn enforce_hard_limits(&mut self, size_limit: u64, chunk_limit: u64) -> Result<()> {
        if let Some(bundle) = self.active.as_ref() {
            let stats = bundle.stats();
            if stats.physical_bytes > size_limit || stats.chunk_count > chunk_limit {
                let _ = self.seal_active()?;
            }
        }
        Ok(())
    }
}

fn discover_existing_packs(layout: &RepositoryLayout, shard: u8) -> Result<(BTreeSet<u32>, u32)> {
    let dir = layout.ensure_shard_dir(shard)?;
    let mut packs = BTreeSet::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Some(id) = parse_pack_id(entry.path(), PACK_DATA_SUFFIX) {
            packs.insert(id);
        }
    }
    let next_pack_id = packs.iter().copied().max().unwrap_or(0).saturating_add(1);
    Ok((packs, next_pack_id.max(1)))
}

fn locate_in_pack_ids(
    layout: &RepositoryLayout,
    shard: u8,
    pack_ids: &[u32],
    hash: &ChunkHash,
) -> Result<Option<(IndexEntry, PathBuf)>> {
    for &pack_id in pack_ids.iter().rev() {
        let (dat_path, idx_path) = layout.pack_paths(shard, pack_id)?;
        if !idx_path.exists() || !dat_path.exists() {
            continue;
        }
        let snapshot = IndexSnapshot::open(&idx_path)?;
        if let Some(entry) = snapshot.find(hash) {
            return Ok(Some((entry.clone(), dat_path)));
        }
    }
    Ok(None)
}

fn parse_pack_id(path: PathBuf, suffix: &str) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    if !name.starts_with(PACK_FILE_PREFIX) || !name.ends_with(suffix) {
        return None;
    }
    let number = &name[PACK_FILE_PREFIX.len()..name.len() - suffix.len()];
    number.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{Compression, compute_chunk_hash};

    fn generate_chunks_for_same_shard(count: usize, len: usize) -> (u8, Vec<Vec<u8>>) {
        assert!(len >= 4, "len must allow embedding counter");
        let mut target = None;
        let mut chunks = Vec::with_capacity(count);
        let mut counter: u32 = 0;
        while chunks.len() < count {
            let mut data = vec![0u8; len];
            let counter_bytes = counter.to_le_bytes();
            let copy_len = counter_bytes.len().min(len);
            data[..copy_len].copy_from_slice(&counter_bytes[..copy_len]);
            let hash = compute_chunk_hash(&data);
            let shard = hash[0];
            if target.map_or(true, |t| t == shard) {
                target = Some(shard);
                chunks.push(data);
            }
            counter = counter.wrapping_add(1);
            if counter == 0 {
                panic!("failed to find enough chunks for the same shard");
            }
        }
        (target.expect("shard assigned"), chunks)
    }

    #[test]
    fn manager_write_and_read_roundtrip() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RepositoryManager::new(temp_dir.path())?;
        let record = manager.write_chunk(b"manager data", Compression::Lz4)?;
        let bytes = manager.read_chunk(&record.hash)?;
        assert_eq!(bytes, b"manager data");
        manager.seal_all()?;
        Ok(())
    }

    #[test]
    fn seal_specific_bundle() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RepositoryManager::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        let record = manager.write_chunk(b"bundle control", Compression::None)?;
        let shard = record.hash[0];
        let pack_id = {
            let guard = manager.shards[shard as usize].read().unwrap();
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id)
                .expect("active bundle must exist")
        };
        let sealed = manager.seal_bundle(shard, pack_id)?;
        assert!(sealed);
        {
            let guard = manager.shards[shard as usize].read().unwrap();
            assert!(guard.active.is_none());
        }
        let _ = manager.write_chunk(b"next bundle chunk", Compression::None)?;
        Ok(())
    }

    #[test]
    fn seal_due_to_chunk_limit_before_write() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RepositoryManager::with_limits(temp_dir.path(), u64::MAX, u64::MAX, 1)?;
        let (_, chunks) = generate_chunks_for_same_shard(3, 32);
        let first = manager.write_chunk(&chunks[0], Compression::None)?;
        let shard = first.hash[0];
        let initial_pack_id = {
            let guard = manager.shards[shard as usize].read().unwrap();
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id)
                .expect("active bundle must exist")
        };
        manager.write_chunk(&chunks[1], Compression::None)?;
        manager.write_chunk(&chunks[2], Compression::None)?;
        let guard = manager.shards[shard as usize].read().unwrap();
        assert!(
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id > initial_pack_id)
                .unwrap_or(false)
        );
        Ok(())
    }

    #[test]
    fn seal_due_to_size_limit_before_write() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = RepositoryManager::with_limits(temp_dir.path(), u64::MAX, 64, u64::MAX)?;
        let (_, chunks) = generate_chunks_for_same_shard(2, 80);
        let first = manager.write_chunk(&chunks[0], Compression::None)?;
        let shard = first.hash[0];
        let initial_pack_id = {
            let guard = manager.shards[shard as usize].read().unwrap();
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id)
                .expect("active bundle must exist")
        };
        manager.write_chunk(&chunks[1], Compression::None)?;
        let guard = manager.shards[shard as usize].read().unwrap();
        assert!(
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id > initial_pack_id)
                .unwrap_or(false)
        );
        Ok(())
    }
}
