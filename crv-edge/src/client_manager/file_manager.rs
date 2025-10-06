use std::fs::{self};
use std::io::{self};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

use crate::client_manager::file::{FileMetadata};
use crate::client_manager::block_manager::BlockManager;

pub struct FileManager {
    file_pool: HashMap<PathBuf, FileMetadata>,
    block_manager: BlockManager,
}

impl FileManager {
    pub fn new(block_manager: BlockManager) -> io::Result<Self> {
        Ok(FileManager {
            file_pool: HashMap::new(),
            block_manager: block_manager,
        })
    }

    pub fn submit_file_local(&mut self, file_path: &Path, changelist: u32) -> io::Result<&FileMetadata> {
        let block_hashes = self.block_manager.create_blocks_at_path(file_path)?; 

        let res = self.file_pool.entry(file_path.to_path_buf()).or_insert(FileMetadata::new(file_path));
        res.add_revision(changelist, block_hashes);

        Ok(res)
    }

    /// 如果 path 是文件，则获取其 metadata 并切换至最新 revision；
    /// 如果是目录，则递归处理目录下所有文件。
    pub fn get_latest(&mut self, path: &Path) -> io::Result<()> {
        if path.is_file() {
            // 文件：直接从 pool 里取出并切换至最新 revision
            if let Some(meta) = self.file_pool.get_mut(path) {
                let latest_rev = (meta.revisions.len() - 1) as u32;
                meta.switch_revision(latest_rev, &mut self.block_manager);
            }
            Ok(())
        } else if path.is_dir() {
            // 目录：递归处理
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let entry_path = entry.path();
                self.get_latest(&entry_path)?;
            }
            Ok(())
        } else {
            // 既不是文件也不是目录，忽略
            Ok(())
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_file_processing() -> io::Result<()> {
//         let temp_dir = tempdir()?;
//         let store_path = temp_dir.path().join("blocks");
//         let test_file = temp_dir.path().join("test.txt");
        
//         // 创建测试文件
//         let test_data = b"Hello, World! This is a test file.".repeat(1000);
//         fs::write(&test_file, &test_data)?;

//         // 创建文件管理器并处理文件
//         let mut file_manager = FileManager::new(&store_path)?;
//         let file_metadata = file_manager.submit_file(
//             test_file.to_str().unwrap(),
//         )?;

//         // 验证块数量
//         let expected_blocks = (test_data.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;
//         assert_eq!(file_metadata.block_hashes.len(), expected_blocks);

//         // 读取并验证文件内容
//         let reconstructed_data = file_manager.read_file(&file_metadata)?;
//         assert_eq!(reconstructed_data, test_data);

//         Ok(())
//     }

//     #[test]
//     fn test_duplicate_blocks() -> io::Result<()> {
//         let temp_dir = tempdir()?;
//         let store_path = temp_dir.path().join("blocks");
//         let test_file1 = temp_dir.path().join("test1.txt");
//         let test_file2 = temp_dir.path().join("test2.txt");
        
//         // 创建两个包含相同数据的测试文件
//         let test_data = b"Hello, World! This is a test file.".repeat(1000);
//         fs::write(&test_file1, &test_data)?;
//         fs::write(&test_file2, &test_data)?;

//         // 创建文件管理器并处理两个文件
//         let mut file_manager = FileManager::new(&store_path)?;
//         let file_metadata1 = file_manager.process_file(
//             test_file1.to_str().unwrap(),
//         )?;
//         let file_metadata2 = file_manager.process_file(
//             test_file2.to_str().unwrap(),
//         )?;

//         // 验证两个文件生成的块完全相同
//         assert_eq!(file_metadata1.block_hashes.len(), file_metadata2.block_hashes.len());
//         assert_eq!(file_metadata1.block_hashes, file_metadata2.block_hashes);
//         assert_eq!(file_metadata1.total_size, file_metadata2.total_size);

//         // 验证块池中没有重复的块
//         assert_eq!(file_manager.block_manager.get_cached_block_count(), file_metadata1.block_hashes.len());

//         Ok(())
//     }
// }
