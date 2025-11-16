use crate::client_manager::block_manager::BlockManager;
use std::path::{Path, PathBuf};

pub struct FileRevision {
    pub changelist_id: u32,
    /// 该版本引用的所有块的哈希
    pub block_hashes: Vec<u64>,
}

impl FileRevision {
    pub fn new(changelist_id: u32, block_hashes: Vec<u64>) -> Self {
        FileRevision {
            changelist_id,
            block_hashes,
        }
    }
}

pub struct FileMetadata {
    pub path: PathBuf,
    pub current_revision: u32,
    pub revisions: Vec<FileRevision>,
}

impl FileMetadata {
    pub fn new(path: &Path) -> Self {
        FileMetadata {
            path: path.to_path_buf(),
            current_revision: 0,
            revisions: Vec::new(),
        }
    }

    pub fn add_revision(&mut self, changelist_id: u32, block_hashes: Vec<u64>) {
        let revision = FileRevision::new(changelist_id, block_hashes);
        self.revisions.push(revision);
    }

    pub fn delete_revision(&mut self, changelist_id: u32) {
        self.revisions
            .retain(|rev| rev.changelist_id != changelist_id);
    }

    pub fn switch_revision(&mut self, revision_index: u32, block_manager: &mut BlockManager) {
        if revision_index >= self.revisions.len() as u32 {
            return;
        }

        if self.current_revision == revision_index {
            return;
        }

        self.current_revision = revision_index;

        // 获取流式读取器
        let mut reader = block_manager
            .get_block_content_by_hashs(
                self.revisions[revision_index as usize].block_hashes.clone(),
            )
            .expect("获取块内容失败");

        // 创建或打开目标文件
        let mut file = std::fs::File::create(&self.path).expect("创建文件失败");

        // 流式复制数据
        std::io::copy(&mut reader, &mut file).expect("写入文件失败");
    }
}
