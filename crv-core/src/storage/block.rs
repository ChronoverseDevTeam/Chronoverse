use std::io;
use std::path::{Path, PathBuf};
use twox_hash::xxh3::hash64;
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

pub const BLOCK_SIZE: usize = 4096; // 4KB blocks

#[derive(Debug, PartialEq, Eq)]
pub struct BlockMetadata {
    pub offset: u64,
    pub size: usize,
    pub hash: u64,
    pub compressed_size: usize,
}

impl BlockMetadata {
    pub fn new(data: &[u8]) -> io::Result<Self> {
        let hash = hash64(data);
        
        // 压缩数据以获取压缩后的大小
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        let compressed_data = encoder.finish()?;

        Ok(BlockMetadata {
            offset: 0,  // 这个值会在处理文件时设置
            size: data.len(),
            hash,
            compressed_size: compressed_data.len(),
        })
    }

    pub fn get_path(&self, store_path: &Path) -> PathBuf {
        let hash_str = format!("{:016x}", self.hash);
        let (dir1, rest) = hash_str.split_at(2);
        let (dir2, filename) = rest.split_at(2);
        store_path.join(dir1).join(dir2).join(filename)
    }
}