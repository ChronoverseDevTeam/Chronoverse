use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::config::holder::get_or_init_config;
use crv_core::repository::compute_chunk_hash;
use thiserror::Error;

/// Chunk 缓存相关错误
#[derive(Debug, Error)]
pub enum ChunkCacheError {
    #[error("invalid chunk hash: {0}")]
    InvalidChunkHash(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("hash mismatch: expected {expected}, actual {actual}")]
    HashMismatch { expected: String, actual: String },
}

pub type ChunkCacheResult<T> = Result<T, ChunkCacheError>;

/// 负责在本地临时目录中缓存用户上传的文件 chunk。
///
/// - 根目录默认位于 `ConfigEntity.repository_path` 的 `cache/chunks` 子目录中；
/// - 按 chunk_hash 的前两位进行分片存储，避免单目录过大；
/// - 支持基于 offset 的追加写入，便于与 `UploadFileChunkReq` 对应。
#[derive(Debug, Clone)]
pub struct ChunkCache {
    root: PathBuf,
}

impl ChunkCache {
    /// 从全局配置构造默认的 ChunkCache 实例。
    ///
    /// 缓存目录为：`{repository_path}/cache/chunks`
    pub fn from_config() -> ChunkCacheResult<Self> {
        let cfg = get_or_init_config();
        let mut root = PathBuf::from(&cfg.repository_path);
        root.push("cache");
        root.push("chunks");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// 使用自定义根目录构造 ChunkCache。
    pub fn new(root: impl Into<PathBuf>) -> ChunkCacheResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// 给定 chunk_hash，返回其在缓存目录中的完整路径。
    fn chunk_path(&self, chunk_hash: &str) -> ChunkCacheResult<PathBuf> {
        let hash = chunk_hash.trim();
        if hash.len() < 2 {
            return Err(ChunkCacheError::InvalidChunkHash(hash.to_string()));
        }
        let shard = &hash[0..2];
        let mut path = self.root.clone();
        path.push(shard);
        path.push(hash);
        Ok(path)
    }

    /// 向缓存中追加写入 chunk 数据的一部分。
    ///
    /// - `offset` 为 chunk 内部偏移（字节），要求与当前文件长度一致；
    /// - 如文件不存在则会自动创建；
    /// - 该函数不负责校验 hash 与内容是否匹配，仅负责可靠落盘。
    pub fn append_chunk_part(
        &self,
        chunk_hash: &str,
        offset: u64,
        data: &[u8],
    ) -> ChunkCacheResult<()> {
        let path = self.chunk_path(chunk_hash)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;

        self.append_at_offset(file, offset, data)
    }

    fn append_at_offset(
        &self,
        mut file: std::fs::File,
        offset: u64,
        data: &[u8],
    ) -> ChunkCacheResult<()> {
        let current_len = file.metadata()?.len();
        if current_len != offset {
            return Err(ChunkCacheError::InvalidChunkHash(format!(
                "offset mismatch: current_len = {current_len}, offset = {offset}"
            )));
        }
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        file.flush()?;
        Ok(())
    }

    /// 使用 crv-core 中封装好的 compute_chunk_hash 计算 Blake3，并转为 hex 字符串。
    fn compute_hash_hex(data: &[u8]) -> String {
        let hash = compute_chunk_hash(data);
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// 判断指定 chunk 是否已经完整缓存，并主动校验内容哈希。
    ///
    /// - 返回 Ok(true) 代表文件存在且内容哈希与 chunk_hash 一致；
    /// - 返回 Ok(false) 代表文件不存在；
    /// - 返回 Err(HashMismatch) 代表文件存在但内容与 chunk_hash 不匹配。
    pub fn has_chunk(&self, chunk_hash: &str) -> ChunkCacheResult<bool> {
        let path = self.chunk_path(chunk_hash)?;
        if !path.exists() {
            return Ok(false);
        }

        let mut file = OpenOptions::new().read(true).open(&path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let actual_hex = Self::compute_hash_hex(&buf);
        let expected_hex = chunk_hash.trim().to_lowercase();
        if actual_hex != expected_hex {
            return Err(ChunkCacheError::HashMismatch {
                expected: expected_hex,
                actual: actual_hex,
            });
        }

        Ok(true)
    }

    /// 读取完整 chunk 内容到内存中，并校验内容哈希。
    pub fn read_chunk(&self, chunk_hash: &str) -> ChunkCacheResult<Vec<u8>> {
        let path = self.chunk_path(chunk_hash)?;
        let mut file = OpenOptions::new().read(true).open(&path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let actual_hex = Self::compute_hash_hex(&buf);
        let expected_hex = chunk_hash.trim().to_lowercase();
        if actual_hex != expected_hex {
            return Err(ChunkCacheError::HashMismatch {
                expected: expected_hex,
                actual: actual_hex,
            });
        }

        Ok(buf)
    }

    /// 获取指定 chunk 在缓存目录中的路径。
    ///
    /// 注意：该函数不会检查文件是否真实存在。
    pub fn chunk_path_unchecked(&self, chunk_hash: &str) -> ChunkCacheResult<PathBuf> {
        self.chunk_path(chunk_hash)
    }

    /// 删除指定 chunk 的缓存文件（若存在）。
    pub fn remove_chunk(&self, chunk_hash: &str) -> ChunkCacheResult<()> {
        let path = self.chunk_path(chunk_hash)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// 清空整个缓存目录（慎用）。
    pub fn clear_all(&self) -> ChunkCacheResult<()> {
        if self.root.exists() {
            for entry in fs::read_dir(&self.root)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
            }
        }
        Ok(())
    }
}

