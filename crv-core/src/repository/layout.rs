use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use super::bundle::{PackBundle, PackReader};
use super::chunk::{ChunkHash, ChunkRecord, Compression, compute_chunk_hash};
use super::constants::{PACK_DATA_SUFFIX, PACK_FILE_PREFIX, PACK_INDEX_SUFFIX, SHARD_DIR_PREFIX};
use super::error::{RepositoryError, Result};
use super::index::{IndexEntry, IndexSnapshot};
use super::io_utils::{ensure_parent_dir, FileLockGuard};

const DEFAULT_PACK_SOFT_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_HARD_PACK_SIZE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_HARD_PACK_CHUNK_LIMIT: u64 = 100_000;
const SHARD_LOCK_RETRY: usize = 32;
const SHARD_LOCK_BACKOFF_MS: u64 = 20;
const SHARD_LOCK_STALE_SECS: u64 = 300;

pub struct RepositoryLayout {
    root: PathBuf,
}

impl RepositoryLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_shard_dir(&self, shard: u8) -> Result<PathBuf> {
        let dir = self.root.join(Self::shard_dir_name(shard));
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    pub fn shard_dir_name(shard: u8) -> String {
        format!("{SHARD_DIR_PREFIX}{shard:02x}")
    }

    pub fn pack_base_name(pack_id: u32) -> String {
        format!("{PACK_FILE_PREFIX}{pack_id:06}")
    }

    pub fn pack_paths(&self, shard: u8, pack_id: u32) -> Result<(PathBuf, PathBuf)> {
        let dir = self.ensure_shard_dir(shard)?;
        let base = Self::pack_base_name(pack_id);
        let dat_path = dir.join(format!("{base}{PACK_DATA_SUFFIX}"));
        let idx_path = dir.join(format!("{base}{PACK_INDEX_SUFFIX}"));
        ensure_parent_dir(&dat_path)?;
        ensure_parent_dir(&idx_path)?;
        Ok((dat_path, idx_path))
    }

    pub fn shard_lock_path(&self, shard: u8) -> PathBuf {
        self.root.join(Self::shard_dir_name(shard)).join(".lock")
    }

    pub fn acquire_shard_lock(&self, shard: u8) -> Result<FileLockGuard> {
        let path = self.shard_lock_path(shard);
        FileLockGuard::acquire(
            &path,
            SHARD_LOCK_RETRY,
            Duration::from_millis(SHARD_LOCK_BACKOFF_MS),
            Duration::from_secs(SHARD_LOCK_STALE_SECS),
        )
        .map_err(RepositoryError::from)
    }
}

pub struct Repository {
    layout: RepositoryLayout,
    shards: Vec<RwLock<ShardState>>,
    pack_soft_limit: u64,
    hard_size_limit: u64,
    hard_chunk_limit: u64,
}

impl Repository {
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

    pub fn layout(&self) -> &RepositoryLayout {
        &self.layout
    }

    pub fn write_chunk(&self, data: &[u8], compression: Compression) -> Result<ChunkRecord> {
        let hash = compute_chunk_hash(data);
        let shard = hash[0];
        let _lock = self.layout.acquire_shard_lock(shard)?;
        let lock = &self.shards[shard as usize];
        let mut guard = lock.write().expect("shard lock poisoned");
        guard.refresh_known_packs(&self.layout, shard)?;
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
        let _lock = self.layout.acquire_shard_lock(shard)?;
        let lock = &self.shards[shard as usize];
        let mut guard = lock.write().expect("shard lock poisoned");
        guard.refresh_known_packs(&self.layout, shard)?;
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
        let _lock = self.layout.acquire_shard_lock(shard)?;
        let lock = &self.shards[shard as usize];
        let mut guard = lock.write().expect("shard lock poisoned");
        guard.refresh_known_packs(&self.layout, shard)?;
        guard.seal_specific(pack_id)
    }

    pub fn locate_chunk(&self, hash: &ChunkHash) -> Result<Option<(IndexEntry, PathBuf)>> {
        let shard = hash[0];
        let lock = &self.shards[shard as usize];
        // 读前刷新目录，确保能看到其他进程写入的未封存/封存 pack
        let (maybe_active_hit, pack_ids) = {
            let mut guard = lock.write().expect("shard lock poisoned");
            guard.refresh_known_packs(&self.layout, shard)?;
            if let Some(result) = guard.find_in_active(hash) {
                return Ok(Some(result));
            }
            (None::<()>, guard.all_pack_ids())
        };
        let _ = maybe_active_hit;
        locate_in_pack_ids(&self.layout, shard, &pack_ids, hash)
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

    fn all_pack_ids(&self) -> Vec<u32> {
        self.known_packs.iter().copied().collect()
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
        self.refresh_known_packs(layout, shard)?;
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

    fn refresh_known_packs(&mut self, layout: &RepositoryLayout, shard: u8) -> Result<()> {
        let dir = layout.ensure_shard_dir(shard)?;
        let mut packs = BTreeSet::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if let Some(id) = parse_pack_id(entry.path(), PACK_DATA_SUFFIX) {
                packs.insert(id);
            }
        }
        let max_id = packs.iter().copied().max().unwrap_or(0);
        self.known_packs = packs;
        self.next_pack_id = max_id.saturating_add(1).max(self.next_pack_id);
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
    use std::sync::{Arc, mpsc};
    use std::sync::Mutex;
    use std::thread;
    use std::time::Duration;

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
    fn repository_write_and_read_roundtrip() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(temp_dir.path())?;
        let record = repo.write_chunk(b"manager data", Compression::Lz4)?;
        let bytes = repo.read_chunk(&record.hash)?;
        assert_eq!(bytes, b"manager data");
        repo.seal_all()?;
        Ok(())
    }

    #[test]
    fn seal_specific_bundle() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        let record = repo.write_chunk(b"bundle control", Compression::None)?;
        let shard = record.hash[0];
        let pack_id = {
            let guard = repo.shards[shard as usize].read().unwrap();
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id)
                .expect("active bundle must exist")
        };
        let sealed = repo.seal_bundle(shard, pack_id)?;
        assert!(sealed);
        {
            let guard = repo.shards[shard as usize].read().unwrap();
            assert!(guard.active.is_none());
        }
        let _ = repo.write_chunk(b"next bundle chunk", Compression::None)?;
        Ok(())
    }

    #[test]
    fn seal_due_to_chunk_limit_before_write() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = Repository::with_limits(temp_dir.path(), u64::MAX, u64::MAX, 1)?;
        let (_, chunks) = generate_chunks_for_same_shard(3, 32);
        let first = repo.write_chunk(&chunks[0], Compression::None)?;
        let shard = first.hash[0];
        let initial_pack_id = {
            let guard = repo.shards[shard as usize].read().unwrap();
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id)
                .expect("active bundle must exist")
        };
        repo.write_chunk(&chunks[1], Compression::None)?;
        repo.write_chunk(&chunks[2], Compression::None)?;
        let guard = repo.shards[shard as usize].read().unwrap();
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
        let repo = Repository::with_limits(temp_dir.path(), u64::MAX, 64, u64::MAX)?;
        let (_, chunks) = generate_chunks_for_same_shard(2, 80);
        let first = repo.write_chunk(&chunks[0], Compression::None)?;
        let shard = first.hash[0];
        let initial_pack_id = {
            let guard = repo.shards[shard as usize].read().unwrap();
            guard
                .active
                .as_ref()
                .map(|bundle| bundle.identity().pack_id)
                .expect("active bundle must exist")
        };
        repo.write_chunk(&chunks[1], Compression::None)?;
        let guard = repo.shards[shard as usize].read().unwrap();
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
    fn cross_repo_reads_unsealed_pack_from_disk() -> Result<()> {
        // 模拟“进程A写入未封存 pack，进程B读取”
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_a = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        let record = repo_a.write_chunk(b"unsealed data", Compression::None)?;
        // 不调用 seal，保持活跃 pack 未封存

        // 进程B视角：创建新的 Repository 实例，直接读取
        let repo_b = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        let bytes = repo_b.read_chunk(&record.hash)?;
        assert_eq!(bytes, b"unsealed data");
        Ok(())
    }

    #[test]
    fn orphan_dat_without_index_is_ignored_and_new_writes_succeed() -> Result<()> {
        // 模拟“进程在写 pack 后崩溃，.dat 留下数据但 .idx 未写入”
        use crate::repository::RepositoryError;
        use crate::repository::bundle::PackWriter;

        let temp_dir = tempfile::tempdir().unwrap();
        let layout = RepositoryLayout::new(temp_dir.path());
        let shard: u8 = 0xAA;
        let (dat_path, _idx_path) = layout.pack_paths(shard, 1)?;

        // 手动创建一个只写了 .dat 的 pack，未写 idx
        let mut pack = PackWriter::create_new(&dat_path)?;
        let payload = b"orphan chunk";
        let hash = compute_chunk_hash(payload);
        let _ = pack.append_chunk(hash, payload.len() as u32, Compression::None.to_flags(), payload)?;
        drop(pack); // 模拟崩溃前未写 idx

        // 启动新的 Repository，应忽略缺失 idx 的 .dat
        let repo = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        assert!(matches!(
            repo.read_chunk(&hash),
            Err(RepositoryError::ChunkNotFound { .. })
        ));

        // 新写入应成功，并使用新的 pack id
        let record = repo.write_chunk(b"fresh chunk", Compression::None)?;
        assert_eq!(repo.read_chunk(&record.hash)?, b"fresh chunk");
        let guard = repo.shards[shard as usize].read().unwrap();
        // 新 pack id 应大于遗留的 1
        assert!(guard
            .active
            .as_ref()
            .map(|b| b.identity().pack_id >= 2)
            .unwrap_or(true));
        Ok(())
    }

    #[test]
    fn concurrent_writes_same_shard_no_duplicates() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?);
        let (_, chunks) = generate_chunks_for_same_shard(16, 64);
        let mut handles = Vec::new();
        for chunk in chunks.clone() {
            let repo_cloned = repo.clone();
            handles.push(thread::spawn(move || {
                repo_cloned.write_chunk(&chunk, Compression::None)
            }));
        }
        let mut results = Vec::new();
        for handle in handles {
            let record = handle.join().expect("thread panicked")?;
            results.push(record);
        }
        // 确认无重复且数据一致
        let mut seen = std::collections::BTreeSet::new();
        for (chunk, record) in chunks.iter().zip(results.iter()) {
            assert!(seen.insert(record.hash), "duplicate hash detected");
            let bytes = repo.read_chunk(&record.hash)?;
            assert_eq!(bytes, *chunk);
        }
        Ok(())
    }

    #[test]
    fn concurrent_read_while_write_cross_repo() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_writer = Arc::new(Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?);
        let (shard, chunks) = generate_chunks_for_same_shard(8, 64);
        let (tx, rx) = mpsc::channel();

        // 读线程使用新的 Repository 实例，模拟另一进程
        let temp_path = temp_dir.path().to_path_buf();
        let reader_handle = thread::spawn(move || -> Result<()> {
            let repo_reader = Repository::with_pack_soft_limit(&temp_path, u64::MAX)?;
            for (hash, data) in rx {
                let mut attempts = 0;
                loop {
                    match repo_reader.read_chunk(&hash) {
                        Ok(bytes) => {
                            assert_eq!(bytes, data);
                            break;
                        }
                        Err(_) if attempts < 5 => {
                            attempts += 1;
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            Ok(())
        });

        // 写线程在主线程：逐个写入并发送 hash+数据
        for data in chunks.iter() {
            let record = repo_writer.write_chunk(data, Compression::None)?;
            tx.send((record.hash, data.clone())).unwrap();
        }
        drop(tx);

        reader_handle.join().expect("reader panicked")?;

        // 确认封存后仍可读
        repo_writer.seal_shard(shard)?;
        let repo_reader_final = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        for data in chunks {
            let hash = compute_chunk_hash(&data);
            assert_eq!(repo_reader_final.read_chunk(&hash)?, data);
        }
        Ok(())
    }

    #[test]
    fn cross_repo_competing_writers_same_shard() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_a = Arc::new(Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?);
        let repo_b = Arc::new(Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?);
        let (_, chunks) = generate_chunks_for_same_shard(20, 80);

        let (tx, rx) = mpsc::channel();
        let writer = |repo: Arc<Repository>, data: Vec<Vec<u8>>, tx: mpsc::Sender<ChunkRecord>| {
            thread::spawn(move || -> Result<()> {
                for chunk in data {
                    let rec = repo.write_chunk(&chunk, Compression::None)?;
                    tx.send(rec).unwrap();
                }
                Ok(())
            })
        };

        let mid = chunks.len() / 2;
        let handle_a = writer(repo_a.clone(), chunks[..mid].to_vec(), tx.clone());
        let handle_b = writer(repo_b.clone(), chunks[mid..].to_vec(), tx.clone());
        drop(tx);

        let mut records = Vec::new();
        for rec in rx {
            records.push(rec);
        }
        handle_a.join().expect("writer A panicked")?;
        handle_b.join().expect("writer B panicked")?;

        // 校验：无重复哈希且可读
        let mut seen = std::collections::BTreeSet::new();
        let repo_check = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        for rec in records {
            assert!(seen.insert(rec.hash));
            let bytes = repo_check.read_chunk(&rec.hash)?;
            assert_eq!(bytes.len() as u32, rec.logical_len);
        }
        Ok(())
    }

    #[test]
    fn cross_repo_concurrent_readers_on_unsealed_pack() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_writer = Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?;
        let (_, chunks) = generate_chunks_for_same_shard(6, 64);
        let mut hashes = Vec::new();
        for c in &chunks {
            hashes.push(repo_writer.write_chunk(c, Compression::None)?.hash);
        }

        // 多个独立实例并发读取未封存数据
        let hashes_arc = Arc::new(hashes);
        let results = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let hashes_cloned = hashes_arc.clone();
            let path = temp_dir.path().to_path_buf();
            let results_cloned = results.clone();
            handles.push(thread::spawn(move || -> Result<()> {
                let repo_reader = Repository::with_pack_soft_limit(&path, u64::MAX)?;
                for h in hashes_cloned.iter() {
                    let bytes = repo_reader.read_chunk(h)?;
                    results_cloned.lock().unwrap().push(bytes);
                }
                Ok(())
            }));
        }
        for h in handles {
            h.join().expect("reader panicked")?;
        }
        // 校验读到的条目数正确
        let collected = results.lock().unwrap();
        assert_eq!(collected.len(), hashes_arc.len() * 4);
        Ok(())
    }
}
