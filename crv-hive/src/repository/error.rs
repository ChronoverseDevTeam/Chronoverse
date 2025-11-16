use std::io;
use std::path::PathBuf;

use thiserror::Error;

use super::chunk::ChunkHash;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("I/O错误: {0}")]
    Io(#[from] io::Error),
    #[error("文件 magic 不匹配，期望 {expected:#010x} 实际 {actual:#010x}")]
    InvalidMagic { expected: u32, actual: u32 },
    #[error("文件版本不受支持，期望 {expected:#06x} 实际 {actual:#06x}")]
    InvalidVersion { expected: u16, actual: u16 },
    #[error("保留字段必须为0")]
    ReservedNonZero,
    #[error("索引条目未按哈希升序排序")]
    IndexOutOfOrder,
    #[error("索引条目数量超出支持范围")]
    EntryCountOverflow,
    #[error("Chunk 数据长度超出 u32 限制: {0}")]
    ChunkTooLarge(usize),
    #[error("检测到重复的 Chunk Hash: {hash:?}")]
    DuplicateHash { hash: ChunkHash },
    #[error("CRC32 校验失败: {path}")]
    CrcMismatch { path: PathBuf },
    #[error("文件已经封存：{path}")]
    AlreadySealed { path: PathBuf },
    #[error("未知压缩标记: {0}")]
    UnsupportedCompression(u16),
    #[error("读取 chunk 数据时检测到损坏: {0}")]
    Corrupted(&'static str),
}

pub type Result<T> = std::result::Result<T, RepositoryError>;
