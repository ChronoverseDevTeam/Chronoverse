use crate::daemon_server::db::*;
use bincode::{Decode, Encode};
use crv_core::path::basic::{DepotPath, LocalPath, WorkspaceDir, WorkspacePath};
use std::collections::HashMap;

#[derive(Encode, Decode, Clone)]
pub struct FileRevision {
    pub generation: i64,
    pub revision: i64,
}

impl FileRevision {
    pub fn unexists() -> Self {
        FileRevision {
            generation: 0,
            revision: 0,
        }
    }
}

#[derive(Encode, Decode, Clone)]
pub struct FileLocation {
    pub local_path: LocalPath,
    pub workspace_path: WorkspacePath,
    pub depot_path: DepotPath,
}

/// FileGuard 用于自动解锁文件
pub struct FileGuard {
    db_manager: DbManager,
    pub paths: Vec<WorkspacePath>, // paths = add_paths + existing_paths
    pub add_paths: Vec<WorkspacePath>,
    pub existing_paths: Vec<WorkspacePath>,
}

impl FileGuard {
    fn new(
        db_manager: &DbManager,
        paths: Vec<WorkspacePath>,
        add_paths: Vec<WorkspacePath>,
        existing_paths: Vec<WorkspacePath>,
    ) -> Self {
        Self {
            db_manager: db_manager.clone(),
            paths,
            add_paths,
            existing_paths,
        }
    }

    /// 用于提前释放某个文件的锁，不走自动释放。
    /// 提前释放锁可能导致某个指令内部发生对元数据修改的冲突，
    /// 调用方必须明确知道这一点，并保证不存在与 release 并发的对元数据的修改。
    pub fn release(&self, path: &WorkspacePath) {
        let file_meta = self.db_manager.get_file_meta(path);
        if file_meta.is_err() {
            println!(
                "Meet error {:?} when get meta of file {}",
                file_meta.err(),
                path.to_custom_string()
            );
            return;
        }
        let file_meta = file_meta.unwrap();
        if file_meta.is_none() {
            return;
        }
        let mut file_meta = file_meta.unwrap();
        file_meta.busy = false;
        let result = self.db_manager.set_file_meta(path.clone(), file_meta);
        if result.is_err() {
            println!(
                "Meet error {:?} when set meta of file {}",
                result.err(),
                path.to_custom_string()
            );
        }
    }
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        for path in &self.paths {
            let file_meta_result = self.db_manager.get_file_meta(path);
            if file_meta_result.is_err() {
                println!(
                    "Meet exception when get file meta: {:?}",
                    file_meta_result.err()
                );
                return;
            }
            let file_meta = file_meta_result.unwrap();
            if file_meta.is_none() {
                return;
            }
            let mut file_meta = file_meta.unwrap();
            file_meta.busy = false;
            let result = self.db_manager.set_file_meta(path.clone(), file_meta);
            if result.is_err() {
                println!("Meet exception when set file meta: {:?}", result.err());
            }
        }
    }
}

#[derive(Encode, Decode)]
pub struct FileMeta {
    pub location: FileLocation,
    pub current_revision: FileRevision,
    pub busy: bool,
}

impl DbManager {
    pub fn set_file_meta(&self, path: WorkspacePath, meta: FileMeta) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        let bytes = bincode::encode_to_vec(meta, bincode::config::standard())?;
        self.inner.put_cf(cf, path.to_custom_string(), bytes)?;
        Ok(())
    }

    pub fn get_file_meta(&self, path: &WorkspacePath) -> Result<Option<FileMeta>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        match self.inner.get_cf(cf, path.to_custom_string())? {
            Some(bytes) => {
                let meta: FileMeta =
                    bincode::decode_from_slice(&bytes, bincode::config::standard())?.0;
                Ok(Some(meta))
            }
            None => Ok(None),
        }
    }

    pub fn get_file_meta_under_dir(
        &self,
        dir: &WorkspaceDir,
    ) -> Result<Vec<(WorkspacePath, FileMeta)>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        let dir_string = dir.to_custom_string();
        let dir_bytes = dir_string.as_bytes();
        let iter = self.inner.iterator_cf(
            cf,
            IteratorMode::From(dir_bytes, rocksdb::Direction::Forward),
        );

        let mut result = Vec::new();
        for item in iter {
            match item {
                Ok((key, value)) => {
                    if !key.starts_with(dir_bytes) {
                        break;
                    }
                    let workspace_path_string = String::from_utf8_lossy(&key);
                    let workspace_path = WorkspacePath::parse(&workspace_path_string).expect(
                        &format!("Can't parse workspace path {workspace_path_string}"),
                    );
                    let meta: FileMeta =
                        bincode::decode_from_slice(&value, bincode::config::standard())?.0;
                    result.push((workspace_path, meta));
                }
                Err(e) => return Err(DbError::RocksDb(e)),
            }
        }

        return Ok(result);
    }

    /// 对于调用时元数据还不存在的文件，插入占位元数据并加锁；
    /// 对于调用时元数据已存在的文件，将其标记为繁忙。
    /// 这个方法会首先对 add_files 和 existing_files 分别去重，
    /// 并检查两个集合之间是否存在冲突路径，若存在则直接报错。
    pub fn prepare_command(
        &self,
        add_files: &[FileLocation],
        existing_files: &[FileLocation],
    ) -> Result<FileGuard, DbError> {
        let transaction = self.inner.transaction();
        let file_cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));

        let mut add_files_map = HashMap::new();
        for file in add_files {
            add_files_map
                .entry(file.workspace_path.to_custom_string())
                .or_insert(file);
        }

        let mut existing_files_map = HashMap::new();
        for file in existing_files {
            existing_files_map
                .entry(file.workspace_path.to_custom_string())
                .or_insert(file);
        }

        for path in add_files_map.keys() {
            if existing_files_map.contains_key(path) {
                return Err(DbError::Invalid(format!(
                    "File {} exists in both add_files and existing_files.",
                    path
                )));
            }
        }

        let mut prepared_add_files = vec![];
        for (path_string, file) in &add_files_map {
            if transaction.get_cf(file_cf, path_string)?.is_some() {
                println!("File {} already in meta.", path_string);
                continue;
            }

            let file_meta = FileMeta {
                location: (*file).clone(),
                current_revision: FileRevision::unexists(),
                busy: true,
            };

            let bytes = bincode::encode_to_vec(file_meta, bincode::config::standard())?;
            transaction.put_cf(file_cf, path_string, bytes)?;

            prepared_add_files.push(file.workspace_path.clone());
        }

        let mut prepared_existing_files = vec![];
        for (path_string, file) in &existing_files_map {
            let file_meta_bytes = transaction.get_cf(file_cf, path_string)?;

            if file_meta_bytes.is_none() {
                println!("File {} meta unexists.", path_string);
                continue;
            }

            let file_meta_bytes = file_meta_bytes.unwrap();
            let mut file_meta: FileMeta =
                bincode::decode_from_slice(&file_meta_bytes, bincode::config::standard())?.0;

            if file_meta.busy {
                println!("File {} is busy.", path_string);
                continue;
            }

            file_meta.busy = true;

            let bytes = bincode::encode_to_vec(file_meta, bincode::config::standard())?;
            transaction.put_cf(file_cf, path_string, bytes)?;

            prepared_existing_files.push(file.workspace_path.clone());
        }

        let result = transaction.commit();

        let mut prepared_files = prepared_add_files.clone();
        prepared_files.extend(prepared_existing_files.clone());

        return match result {
            Ok(_) => Ok(FileGuard::new(
                self,
                prepared_files,
                prepared_add_files,
                prepared_existing_files,
            )),
            Err(e) => Err(DbError::RocksDb(e)),
        };
    }

    pub fn delete_file_meta(&self, path: &WorkspacePath) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        self.inner.delete_cf(cf, path.to_custom_string())?;
        Ok(())
    }

    pub fn submit_file(&self, path: WorkspacePath, file_meta: FileMeta) -> Result<(), DbError> {
        let transaction = self.inner.transaction();
        let path_string = path.to_custom_string();
        // 将文件从 active file 中移除
        let cf = self
            .inner
            .cf_handle(Self::CF_ACTIVE_FILE)
            .expect(&format!("cf {} must exist", Self::CF_ACTIVE_FILE));
        transaction.delete_cf(cf, &path_string)?;
        // 写入最新 revision 信息
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));

        let bytes = bincode::encode_to_vec(file_meta, bincode::config::standard())?;
        transaction.put_cf(cf, path.to_custom_string(), bytes)?;
        transaction.commit()?;
        Ok(())
    }
}
