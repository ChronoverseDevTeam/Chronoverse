use crate::client_manager::block_manager::BlockManager;
use crate::client_manager::changelist::ChangelistMetadata;
use crate::client_manager::file_manager::FileManager;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// ChangeList 管理器
pub struct WorkSpaceMetadata {
    current_changelist_id: u32,
    changelists: HashMap<u32, ChangelistMetadata>,
    depot_root_path: PathBuf,
    root_path: PathBuf,
    path_mapping: HashMap<PathBuf, PathBuf>,
    file_manager: FileManager,
}

impl WorkSpaceMetadata {
    /// 新建管理器
    pub fn new(
        root_path: &Path,
        depot_root_path: &Path,
        path_mapping: HashMap<PathBuf, PathBuf>,
    ) -> Self {
        let block_store_path = root_path.join(".vcs").join("blocks");
        let depot_store_path = depot_root_path.join(".vcs").join("blocks");
        let block_manager = BlockManager::new(&block_store_path, &depot_store_path).unwrap();

        let file_manager = FileManager::new(block_manager).unwrap();
        WorkSpaceMetadata {
            current_changelist_id: 0,
            changelists: HashMap::new(),
            path_mapping: path_mapping,
            file_manager: file_manager,
            depot_root_path: depot_root_path.to_path_buf(),
            root_path: root_path.to_path_buf(),
        }
    }

    fn get_mapped_path(&self, _abs_path: &Path) -> Option<PathBuf> {
        // TODO: 实现路径映射逻辑
        None
    }

    /// 递归地从映射的 path 获取最新文件并写入当前路径
    pub fn get_latest(&mut self) -> std::io::Result<()> {
        for (_local_path, mapped_path) in &self.path_mapping {
            self.file_manager.get_latest(Path::new(mapped_path))?;
        }
        Ok(())
    }

    pub fn get_next_changelist_id(&mut self) -> u32 {
        self.current_changelist_id += 1;
        self.current_changelist_id
    }

    /// 将文件加入指定 changelist
    pub fn checkout(&mut self, file_path: String, changelist_id: u32) -> std::io::Result<()> {
        let changelist = self
            .changelists
            .entry(changelist_id)
            .or_insert(ChangelistMetadata::new(
                changelist_id,
                Vec::new(),
                String::new(),
            ));
        changelist.file_paths.push(file_path.clone());
        Ok(())
    }

    pub fn submit_changelist(&mut self, changelist_id: u32, desc: String) -> std::io::Result<()> {
        // 先取出文件路径列表，避免在循环中同时持有 &mut self 和 &changelist
        let file_paths = {
            let changelist =
                self.changelists
                    .entry(changelist_id)
                    .or_insert(ChangelistMetadata::new(
                        changelist_id,
                        Vec::new(),
                        String::new(),
                    ));
            changelist.desc = desc;
            changelist.file_paths.clone()
        };

        for file_path in &file_paths {
            self.file_manager
                .submit_file_local(Path::new(file_path), changelist_id)?;
            let mapped_path = self.get_mapped_path(Path::new(file_path)).unwrap();
            fs::write(mapped_path, fs::read(Path::new(file_path))?)?;
        }
        Ok(())
    }

    /// 获取工作空间中的所有文件路径
    pub fn get_file_paths(&self) -> Vec<String> {
        self.path_mapping
            .keys()
            .map(|path| path.to_string_lossy().to_string())
            .collect()
    }
}
