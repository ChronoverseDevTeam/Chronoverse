# Repository 并发与可靠性设计说明（单进程优先）

当前版本已去除跨进程文件锁，默认假设单进程多线程使用；若需跨进程并发，请由上层自行加锁或避免多实例。

## 并发写入与互斥
- **进程内 RwLock**：每个 shard 一个 `RwLock<ShardState>`，写串行、读并发，不同 shard 互不影响。
- **活跃 pack 协调**：写前刷新目录 (`refresh_known_packs`)，分配 pack id，保证同进程内不重复。

相关代码：
```rust
// layout.rs
pub fn write_chunk(&self, data: &[u8], compression: Compression) -> Result<ChunkRecord> {
    let hash = compute_chunk_hash(data);
    let shard = hash[0];
    let lock = &self.shards[shard as usize];
    let mut guard = lock.write().expect("shard lock poisoned");
    guard.refresh_known_packs(&self.layout, shard)?;
    ...
}
```

## 读可见性
- **读前刷新**：`locate_chunk` 会刷新目录并扫描全部 pack（含未封存），确保能看到同进程写入且已落盘的 chunk；跨进程不保证最新。
- **活跃内存索引优先**：同一实例内，写入后的活跃 pack 可立即读；跨实例通过落盘 idx 读取。

相关代码：
```rust
// layout.rs
pub fn locate_chunk(&self, hash: &ChunkHash) -> Result<Option<(IndexEntry, PathBuf)>> {
    let shard = hash[0];
    let lock = &self.shards[shard as usize];
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
```

## 原子性与一致性
- **索引原子落盘**：`.idx` 写入临时文件后原子 `rename`，封存含 CRC，保证“旧文件或完整新文件”。
- **封存流程**：按 RFC 先校验排序，再写 idx CRC、pack CRC。
- **崩溃容忍**：未封存 pack 尾部半写数据不会被索引引用，对外不可见；有测试覆盖“仅 .dat 无 .idx”场景。

相关代码：
```rust
// index.rs：写入临时文件，再原子 rename 覆盖正式文件
fn write_index_file(path: &Path, entries: &[IndexEntry], sealed: bool) -> Result<()> {
    let tmp_path = path.with_extension("tmp");
    if tmp_path.exists() {
        fs::remove_file(&tmp_path)?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .open(&tmp_path)?;
    write_header(&mut file, entry_count)?;
    for entry in entries {
        write_entry(&mut file, entry)?;
    }
    file.flush()?;
    let data_len = file.metadata()?.len();
    if sealed {
        let crc = compute_crc32(&file, data_len)?;
        file.seek(SeekFrom::End(0))?;
        file.write_all(&crc.to_le_bytes())?;
        file.flush()?;
    }
    file.sync_all()?;
    fs::rename(&tmp_path, path)?; // 原子替换
    Ok(())
}
```

## 可靠性与恢复（待扩展）
- 未封存 pack 的尾部清理/idx 重建可由后续恢复子程序实现（扫描 entry、截断非法尾部、重建未封存 idx）。
- 如需进一步缩小崩溃回滚窗口，可以在后续迭代中引入更严格的 fsync 顺序、末尾指针或 WAL 方案；当前实现未启用这些机制。

## 并发读写验证（测试概览）
- 同 shard 多线程写（单进程）：验证无重复哈希且可读。
- 并发读未封存（单进程多实例）：多个实例读取未封存 pack 中的数据。
- 写-读竞态（单进程多实例）：一实例写、另一实例边写边读，含重试等待 idx 刷新。
- 崩溃模拟：仅 `.dat` 无 `.idx` 被忽略，后续写入正常。

