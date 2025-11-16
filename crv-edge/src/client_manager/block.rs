use flate2::{Compression, write::ZlibEncoder};
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use twox_hash::xxh3::hash64;

pub const BLOCK_SIZE: usize = 4096; // 4KB blocks

#[derive(Debug, PartialEq, Eq)]
pub struct BlockMetadata {
    pub offset: u64,
    pub size: usize,
    pub hash: u64,
    pub compressed_size: usize,
    pub ref_count: u64,
}

impl BlockMetadata {
    pub fn new(data: &[u8]) -> io::Result<Self> {
        let hash = hash64(data);

        // 压缩数据以获取压缩后的大小
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        let compressed_data = encoder.finish()?;

        Ok(BlockMetadata {
            offset: 0, // 这个值会在处理文件时设置
            size: data.len(),
            hash,
            compressed_size: compressed_data.len(),
            ref_count: 1,
        })
    }

    pub fn clone(&self) -> Self {
        Self {
            offset: self.offset,
            size: self.size,
            hash: self.hash,
            compressed_size: self.compressed_size,
            ref_count: self.ref_count,
        }
    }

    /// 根据块文件路径逆向计算哈希值
    pub fn hash_from_abs_path(abs_path: &Path) -> io::Result<u64> {
        // 取路径最后三段
        // 去掉扩展名后再拆分路径
        let stem = abs_path.file_stem().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "无法获取文件名（无扩展名）")
        })?;
        let stem_path = Path::new(stem);
        let components: Vec<_> = stem_path.components().collect();
        if components.len() < 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "路径深度不足，无法提取三段",
            ));
        }
        let last_three: Vec<&str> = components
            .iter()
            .rev()
            .take(3)
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        if last_three.len() != 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "路径组件包含非 UTF-8 字符",
            ));
        }

        // 拼接三段并去掉 .block 后缀
        let combined = last_three.into_iter().rev().collect::<String>();

        u64::from_str_radix(&combined, 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "路径中的哈希格式无效"))
    }

    pub fn get_local_path(hash: u64) -> PathBuf {
        let hash_str = format!("{:016x}", hash);
        let (dir1, rest) = hash_str.split_at(2);
        let (dir2, filename) = rest.split_at(2);
        PathBuf::from(dir1).join(dir2).join(filename)
    }

    /// 从指定路径读取块数据，验证大小和哈希
    pub fn read_from_path(path: &Path) -> io::Result<BlockMetadata> {
        let metadata = fs::metadata(path)?;
        let file_size = metadata.len() as usize;

        // 检查文件大小是否超过块的最大大小
        if file_size > BLOCK_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "文件大小超过块的最大大小",
            ));
        }

        let data = fs::read(path)?;
        let hash = hash64(&data);

        // 使用 hash_from_abs_path 方法计算期望的哈希值
        let expected_hash = BlockMetadata::hash_from_abs_path(path)?;

        // 验证哈希值是否一致
        if hash != expected_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "文件内容哈希与文件名不匹配",
            ));
        }

        BlockMetadata::new(&data)
    }
}
