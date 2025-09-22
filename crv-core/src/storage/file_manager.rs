use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use twox_hash::xxh3::hash64;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::Write;
use tempfile::tempdir;
use std::collections::HashMap;
use crate::storage::block::{BlockMetadata, BLOCK_SIZE};

#[derive(Debug)]
pub struct FileMetadata {
    pub block_hashes: Vec<u64>,
    pub path: PathBuf,
    pub total_size: u64,
}

impl FileMetadata {
    pub fn new(block_hashes: Vec<u64>, path: PathBuf, total_size: u64) -> Self {
        FileMetadata {
            block_hashes,
            path,
            total_size,
        }
    }
}

pub struct FileManager {
    store_path: PathBuf,
    file_pool: HashMap<PathBuf, FileMetadata>,
    block_pool: HashMap<u64, BlockMetadata>,
}

impl FileManager {
    pub fn new(store_path: impl Into<PathBuf>) -> io::Result<Self> {
        let store_path = store_path.into();
        fs::create_dir_all(&store_path)?;
        Ok(FileManager {
            store_path,
            file_pool: HashMap::new(),
            block_pool: HashMap::new(),
        })
    }

    pub fn process_file(&mut self, file_path: &str) -> io::Result<FileMetadata> {
        let mut file = File::open(file_path)?;
        let file_size = file.metadata()?.len();
        let mut current_offset = 0;
        let mut block_hashes = Vec::new();

        while current_offset < file_size {
            let bytes_to_read = (file_size - current_offset).min(BLOCK_SIZE as u64) as usize;
            let mut buffer = vec![0; bytes_to_read];
            file.read_exact(&mut buffer)?;

            let block_metadata = BlockMetadata::new(&buffer)?;
            let hash = block_metadata.hash;

            // 首先检查块池中是否已存在该块
            if !self.block_pool.contains_key(&hash) {
                let block_path = block_metadata.get_path(&self.store_path);
                
                // 如果块池中不存在，检查文件系统中是否存在
                if !block_path.exists() {
                    // 文件系统中也不存在，创建新块
                    if let Some(parent) = block_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    
                    // 使用zlib压缩数据
                    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
                    encoder.write_all(&buffer)?;
                    let compressed_data = encoder.finish()?;
                    
                    fs::write(&block_path, &compressed_data)?;
                }
                
                // 将块元数据添加到块池
                self.block_pool.insert(hash, block_metadata);
            }

            block_hashes.push(hash);
            current_offset += bytes_to_read as u64;
        }

        Ok(FileMetadata::new(block_hashes, PathBuf::from(file_path), file_size))
    }

    pub fn read_block(&self, hash: u64) -> io::Result<Vec<u8>> {
        let block_metadata = self.block_pool.get(&hash)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Block not found in pool"))?;
        
        let block_path = block_metadata.get_path(&self.store_path);
        let compressed_data = fs::read(block_path)?;
        
        // 解压数据
        let mut decoder = ZlibDecoder::new(&compressed_data[..]);
        let mut decompressed_data = Vec::new();
        decoder.read_to_end(&mut decompressed_data)?;
        
        Ok(decompressed_data)
    }

    pub fn read_file(&self, file_metadata: &FileMetadata) -> io::Result<Vec<u8>> {
        let mut file_data = Vec::with_capacity(file_metadata.total_size as usize);
        
        for &hash in &file_metadata.block_hashes {
            let block_data = self.read_block(hash)?;
            file_data.extend_from_slice(&block_data);
        }
        
        Ok(file_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_processing() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let store_path = temp_dir.path().join("blocks");
        let test_file = temp_dir.path().join("test.txt");
        
        // 创建测试文件
        let test_data = b"Hello, World! This is a test file.".repeat(1000);
        fs::write(&test_file, &test_data)?;

        // 创建文件管理器并处理文件
        let mut file_manager = FileManager::new(&store_path)?;
        let file_metadata = file_manager.process_file(
            test_file.to_str().unwrap(),
        )?;

        // 验证块数量
        let expected_blocks = (test_data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;
        assert_eq!(file_metadata.block_hashes.len(), expected_blocks);

        // 读取并验证文件内容
        let reconstructed_data = file_manager.read_file(&file_metadata)?;
        assert_eq!(reconstructed_data, test_data);

        Ok(())
    }

    #[test]
    fn test_duplicate_blocks() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let store_path = temp_dir.path().join("blocks");
        let test_file1 = temp_dir.path().join("test1.txt");
        let test_file2 = temp_dir.path().join("test2.txt");
        
        // 创建两个包含相同数据的测试文件
        let test_data = b"Hello, World! This is a test file.".repeat(1000);
        fs::write(&test_file1, &test_data)?;
        fs::write(&test_file2, &test_data)?;

        // 创建文件管理器并处理两个文件
        let mut file_manager = FileManager::new(&store_path)?;
        let file_metadata1 = file_manager.process_file(
            test_file1.to_str().unwrap(),
        )?;
        let file_metadata2 = file_manager.process_file(
            test_file2.to_str().unwrap(),
        )?;

        // 验证两个文件生成的块完全相同
        assert_eq!(file_metadata1.block_hashes.len(), file_metadata2.block_hashes.len());
        assert_eq!(file_metadata1.block_hashes, file_metadata2.block_hashes);
        assert_eq!(file_metadata1.total_size, file_metadata2.total_size);

        // 验证块池中没有重复的块
        assert_eq!(file_manager.block_pool.len(), file_metadata1.block_hashes.len());

        Ok(())
    }
}
