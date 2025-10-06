use std::path::{Path, PathBuf};
use crate::client_manager::block_manager::BlockManager;

pub struct FileRevision {
    pub changelist_id: u32,
    /// 该版本引用的所有块的哈希
    pub block_hashes: Vec<u64>,
}

impl FileRevision {
    pub fn new(changelist_id: u32, block_hashes: Vec<u64>) -> Self {
        FileRevision { changelist_id, block_hashes }
    }
}

pub struct FileMetadata {
    pub path: PathBuf,
    pub current_revision: u32,
    pub revisions: Vec<FileRevision>,
}

impl FileMetadata {
    pub fn new(path: &Path) -> Self {
        FileMetadata { path: path.to_path_buf(), current_revision: 0, revisions: Vec::new() }
    }

    pub fn add_revision(&mut self, changelist_id: u32, block_hashes: Vec<u64>) {
        let revision = FileRevision::new(changelist_id, block_hashes);
        self.revisions.push(revision);
    }

    pub fn delete_revision(&mut self, changelist_id: u32) {
        self.revisions.retain(|rev| rev.changelist_id != changelist_id);
    }

    pub fn switch_revision(&mut self, revision_index: u32, block_manager: &mut BlockManager){
        if revision_index >= self.revisions.len() as u32 {
            return;
        }
        
        if self.current_revision == revision_index {
            return;
        }

        self.current_revision = revision_index;
        let data = block_manager.get_block_content_by_hashs(self.revisions[revision_index as usize].block_hashes.clone());
        std::fs::write(&self.path, data.unwrap()).expect("写入文件失败");
    }
}
