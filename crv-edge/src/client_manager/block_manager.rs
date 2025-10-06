use std::io;
use std::path::{Path, PathBuf};
use std::fs;
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
                    fs::write(&block_path, fs::read(&depot_block_path)?)?;
                    let metadata = BlockMetadata::read_from_path(&block_path)?;
                    self.cache.insert(hash, metadata);
                    Ok(&self.cache[&hash])
                } else {
                    Err(io::Error::new(io::ErrorKind::NotFound, "block not found"))
                }
            }
        }
    }

    fn get_block_content_by_hash(&mut self, hash: u64) -> io::Result<Vec<u8>> {
        if let Err(_) = self.get_block_metadata_by_hash(hash) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "block not found"));
        }
        let block_path = self.store_path.join(BlockMetadata::get_local_path(hash));
        fs::read(&block_path)
    }

    pub fn get_block_content_by_hashs(&mut self, hashs: Vec<u64>) -> io::Result<Vec<u8>> {
        let mut res = Vec::<u8>::new();
        for hash in hashs {
            let data = self.get_block_content_by_hash(hash)?;
            res.extend_from_slice(&data);
        }
        Ok(res)
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
        let data = fs::read(path)?;
        let mut offset = 0;
        let mut hashs = Vec::<u64>::new();
        while offset < data.len() {
            let end = std::cmp::min(offset + BLOCK_SIZE, data.len());
            let block_data = &data[offset..end];
            hashs.push(self.create_single_block(block_data)?);
            offset += BLOCK_SIZE;
        }
        Ok(hashs)
    }
}