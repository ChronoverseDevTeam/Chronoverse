use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::fs::{self, File};
use std::collections::HashMap;
use twox_hash::xxh3::hash64;

use crate::client_manager::block::{BlockMetadata, BLOCK_SIZE};

pub struct BlockManager {
    store_path: PathBuf,
    depot_store_path: PathBuf,
    cache: HashMap<u64, BlockMetadata>,
}

//懒加载block，当访问某个hash后，从本地进行加载，或者从远程加载
impl BlockManager {
    pub fn new(store_path: &Path, depot_path: &Path) -> io::Result<Self> {
        Ok(BlockManager {store_path: store_path.to_path_buf(), cache: HashMap::new(), depot_store_path: depot_path.to_path_buf() })
    }

    pub fn get_block_metadata_by_hash(&mut self, hash: u64) -> io::Result<&BlockMetadata> {
        if self.cache.contains_key(&hash) { 
            return Ok(&self.cache[&hash]);
        }

        let block_path = self.store_path.join(BlockMetadata::get_local_path(hash));
        match BlockMetadata::read_from_path(&block_path) {
            Ok(metadata) => {
                self.cache.insert(hash, metadata);
                Ok(&self.cache[&hash])
            }
            Err(_) => {
                let depot_block_path = self.depot_store_path.join(BlockMetadata::get_local_path(hash));
                if depot_block_path.exists() {
                    // 使用流式复制替代全量读写
                    let mut source = File::open(&depot_block_path)?;
                    let mut dest = File::create(&block_path)?;
                    io::copy(&mut source, &mut dest)?;
                    
                    let metadata = BlockMetadata::read_from_path(&block_path)?;
                    self.cache.insert(hash, metadata);
                    Ok(&self.cache[&hash])
                } else {
                    Err(io::Error::new(io::ErrorKind::NotFound, "block not found"))
                }
            }
        }
    }

    /// 流式读取单个块的内容
    fn get_block_content_by_hash(&mut self, hash: u64) -> io::Result<File> {
        if let Err(_) = self.get_block_metadata_by_hash(hash) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "block not found"));
        }
        let block_path = self.store_path.join(BlockMetadata::get_local_path(hash));
        File::open(&block_path)
    }

    /// 流式读取多个块的内容
    /// 返回一个实现了 Read trait 的 BlockReader，可以按需读取数据
    pub fn get_block_content_by_hashs(&mut self, hashs: Vec<u64>) -> io::Result<BlockReader> {
        // 预先验证所有块是否存在
        for &hash in &hashs {
            self.get_block_metadata_by_hash(hash)?;
        }
        
        Ok(BlockReader {
            block_manager: self,
            hashs,
            current_block: None,
            current_hash_index: 0,
        })
    }

    pub fn create_single_block(&mut self, data: &[u8]) -> io::Result<u64> {
        if data.len() > BLOCK_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "block size exceeds"));
        }

        let hash = hash64(data);
        if let Ok(metadata) = self.get_block_metadata_by_hash(hash) {
            return Ok(metadata.hash);
        }

        let block_path = self.store_path.join(BlockMetadata::get_local_path(hash));
        if let Some(parent) = block_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&block_path, data)?;

        let metadata = BlockMetadata::new(data);
        self.cache.insert(hash, metadata.unwrap());
        Ok(hash)
    }

    pub fn create_blocks_at_path(&mut self, path: &Path) -> io::Result<Vec<u64>> {
        let mut file = File::open(path)?;
        let mut buffer = vec![0; BLOCK_SIZE];
        let mut hashs = Vec::new();

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hashs.push(self.create_single_block(&buffer[..bytes_read])?);
        }
        Ok(hashs)
    }
}

/// 用于流式读取多个块内容的读取器
pub struct BlockReader<'a> {
    block_manager: &'a mut BlockManager,
    hashs: Vec<u64>,
    current_block: Option<File>,
    current_hash_index: usize,
}

impl<'a> Read for BlockReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // 如果当前没有打开的块，或者当前块已读完，尝试打开下一个块
        if self.current_block.is_none() {
            if self.current_hash_index >= self.hashs.len() {
                return Ok(0); // 所有块都读完了
            }
            let hash = self.hashs[self.current_hash_index];
            self.current_block = Some(self.block_manager.get_block_content_by_hash(hash)?);
            self.current_hash_index += 1;
        }

        // 从当前块读取数据
        if let Some(ref mut block) = self.current_block {
            let bytes_read = block.read(buf)?;
            if bytes_read == 0 {
                // 当前块读完了，清除它，下次会读取下一个块
                self.current_block = None;
                // 递归调用以读取下一个块
                return self.read(buf);
            }
            Ok(bytes_read)
        } else {
            Ok(0)
        }
    }
}