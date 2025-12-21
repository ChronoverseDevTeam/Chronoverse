use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use super::bundle::{PackBundle, PackReader};
use super::chunk::{ChunkHash, ChunkRecord, Compression, compute_chunk_hash};
use super::constants::{PACK_DATA_SUFFIX, PACK_FILE_PREFIX, PACK_INDEX_SUFFIX, SHARD_DIR_PREFIX};
use super::error::{RepositoryError, Result};
use super::index::{IndexEntry, IndexSnapshot};
use super::io_utils::ensure_parent_dir;

const DEFAULT_PACK_SOFT_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_HARD_PACK_SIZE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_HARD_PACK_CHUNK_LIMIT: u64 = 100_000;
const INDEX_CACHE_CAPACITY: usize = 128;

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

}

pub struct Repository {
    layout: RepositoryLayout,
    shards: Vec<RwLock<ShardState>>,
    index_cache: Mutex<IndexCache>,
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
            index_cache: Mutex::new(IndexCache::new(INDEX_CACHE_CAPACITY)),
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
        let lock = &self.shards[shard as usize];
        let mut guard = lock
            .write()
            .map_err(|_| RepositoryError::Corrupted("shard lock poisoned"))?;
        guard.enforce_hard_limits(self.hard_size_limit, self.hard_chunk_limit)?;
        if guard.active_contains(&hash) {
            return Err(RepositoryError::DuplicateHash { hash });
        }
        let sealed_pack_ids = guard.sealed_pack_ids();
        if self
            .locate_in_pack_ids(shard, &sealed_pack_ids, &hash)?
            .is_some()
        {
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
        let mut guard = lock
            .write()
            .map_err(|_| RepositoryError::Corrupted("shard lock poisoned"))?;
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
        let mut guard = lock
            .write()
            .map_err(|_| RepositoryError::Corrupted("shard lock poisoned"))?;
        guard.seal_specific(pack_id)
    }

    pub fn locate_chunk(&self, hash: &ChunkHash) -> Result<Option<(IndexEntry, PathBuf)>> {
        let shard = hash[0];
        let lock = &self.shards[shard as usize];
        // 注意，目前 Repo 的设计是只允许一个实例多线程访问的，别搞多进程的情况！！！
        // 无需刷新目录，直接查询活跃 pack，然后索引缓存
        let pack_ids = {
            let guard = lock
                .read()
                .map_err(|_| RepositoryError::Corrupted("shard lock poisoned"))?;
            if let Some(result) = guard.find_in_active(hash) {
                return Ok(Some(result));
            }
            guard.all_pack_ids()
        };
        self.locate_in_pack_ids(shard, &pack_ids, hash)
    }

    fn locate_in_pack_ids(
        &self,
        shard: u8,
        pack_ids: &[u32],
        hash: &ChunkHash,
    ) -> Result<Option<(IndexEntry, PathBuf)>> {
        for &pack_id in pack_ids.iter().rev() {
            if let Some((entry, dat_path)) = self.cached_index_lookup(shard, pack_id, hash)? {
                return Ok(Some((entry, dat_path)));
            }
        }
        Ok(None)
    }

    fn cached_index_lookup(
        &self,
        shard: u8,
        pack_id: u32,
        hash: &ChunkHash,
    ) -> Result<Option<(IndexEntry, PathBuf)>> {
        let snapshot_opt = {
            let mut cache = self
                .index_cache
                .lock()
                .map_err(|_| RepositoryError::Corrupted("index cache lock poisoned"))?;
            cache.get_or_load(&self.layout, shard, pack_id)?
        };
        if let Some(snapshot) = snapshot_opt {
            if let Some(entry) = snapshot.find(hash) {
                let (dat_path, _) = self.layout.pack_paths(shard, pack_id)?;
                if dat_path.exists() {
                    return Ok(Some((entry.clone(), dat_path)));
                }
            }
        }
        Ok(None)
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
        if self.active.is_some() {
            return Ok(self.active.as_mut().expect("active bundle must exist"));
        }

        let pack_id = self.next_pack_id;
        let next = pack_id
            .checked_add(1)
            .ok_or(RepositoryError::PackIdOverflow)?;
        let bundle = PackBundle::create(layout, shard, pack_id)?;
        self.known_packs.insert(pack_id);
        self.active = Some(bundle);
        self.next_pack_id = next;
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

struct IndexCacheEntry {
    snapshot: Arc<IndexSnapshot>,
    modified: std::time::SystemTime,
    len: u64,
}

struct IndexCache {
    map: HashMap<(u8, u32), IndexCacheEntry>,
    order: VecDeque<(u8, u32)>,
    capacity: usize,
}

impl IndexCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    fn touch(&mut self, key: (u8, u32)) {
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key);
    }

    fn insert(&mut self, key: (u8, u32), entry: IndexCacheEntry) {
        self.map.insert(key, entry);
        self.touch(key);
        if self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    fn get_or_load(
        &mut self,
        layout: &RepositoryLayout,
        shard: u8,
        pack_id: u32,
    ) -> Result<Option<Arc<IndexSnapshot>>> {
        let key = (shard, pack_id);
        let (dat_path, idx_path) = layout.pack_paths(shard, pack_id)?;
        if !idx_path.exists() || !dat_path.exists() {
            return Ok(None);
        }
        let meta = idx_path.metadata()?;
        let len = meta.len();
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        if let Some(entry) = self.map.get(&key) {
            if entry.len == len && entry.modified == modified {
                let snap = entry.snapshot.clone();
                self.touch(key);
                return Ok(Some(snap));
            }
        }
        let snapshot = Arc::new(IndexSnapshot::open(&idx_path)?);
        let entry = IndexCacheEntry {
            snapshot: snapshot.clone(),
            modified,
            len,
        };
        self.insert(key, entry);
        Ok(Some(snapshot))
    }
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
    use std::sync::{Arc, mpsc, Mutex};
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
    fn concurrent_read_while_write_single_repo() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?);
        let (shard, chunks) = generate_chunks_for_same_shard(8, 64);
        let (tx, rx) = mpsc::channel();

        let repo_writer = repo.clone();
        let writer = thread::spawn(move || -> Result<()> {
            for data in chunks.iter() {
                let record = repo_writer.write_chunk(data, Compression::None)?;
                tx.send((record.hash, data.clone())).unwrap();
            }
            Ok(())
        });

        let repo_reader = repo.clone();
        let reader = thread::spawn(move || -> Result<()> {
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

        writer.join().expect("writer panicked")?;
        reader.join().expect("reader panicked")?;

        // 写完后封存，确认可读
        repo.seal_shard(shard)?;
        for data in [
            b"post seal check 1".as_ref().to_vec(),
            b"post seal check 2".as_ref().to_vec(),
        ] {
            let record = repo.write_chunk(&data, Compression::None)?;
            assert_eq!(repo.read_chunk(&record.hash)?, data);
        }
        Ok(())
    }

    #[test]
    fn concurrent_readers_on_unsealed_single_repo() -> Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::with_pack_soft_limit(temp_dir.path(), u64::MAX)?);
        let (_, chunks) = generate_chunks_for_same_shard(6, 64);
        let mut hashes = Vec::new();
        for c in &chunks {
            hashes.push(repo.write_chunk(c, Compression::None)?.hash);
        }

        let hashes_arc = Arc::new(hashes);
        let results = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let repo_reader = repo.clone();
            let hashes_cloned = hashes_arc.clone();
            let results_cloned = results.clone();
            handles.push(thread::spawn(move || -> Result<()> {
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
        let collected = results.lock().unwrap();
        assert_eq!(collected.len(), hashes_arc.len() * 4);
        Ok(())
    }
}
