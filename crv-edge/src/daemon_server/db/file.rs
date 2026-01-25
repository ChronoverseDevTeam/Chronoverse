use crate::daemon_server::db::*;
use bincode::{Decode, Encode};
use crv_core::path::basic::{WorkspaceDir, WorkspacePath};

#[derive(Encode, Decode)]
pub struct FileMeta {
    pub latest_revision: String,
}

impl DbManager {
    pub fn set_file_meta(&self, path: WorkspacePath, meta: FileMeta) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        let bytes = bincode::encode_to_vec(meta, bincode::config::standard())?;
        self.inner.put_cf(cf, path.to_string(), bytes)?;
        Ok(())
    }

    pub fn delete_file(&self, path: &WorkspacePath) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        self.inner.delete_cf(cf, path.to_string())?;
        Ok(())
    }

    pub fn get_file_meta(&self, path: &WorkspacePath) -> Result<Option<FileMeta>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        match self.inner.get_cf(cf, path.to_string())? {
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
        let dir_string = dir.to_string();
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

    pub fn submit_file(&self, path: WorkspacePath, latest_revision: String) -> Result<(), DbError> {
        // 将文件从 active file 中移除
        let cf = self
            .inner
            .cf_handle(Self::CF_ACTIVE_FILE)
            .expect(&format!("cf {} must exist", Self::CF_ACTIVE_FILE));
        self.inner.delete_cf(cf, path.to_string())?;
        // 写入最新 revision 信息
        let cf = self
            .inner
            .cf_handle(Self::CF_FILE)
            .expect(&format!("cf {} must exist", Self::CF_FILE));
        let meta = FileMeta { latest_revision };
        let bytes = bincode::encode_to_vec(meta, bincode::config::standard())?;
        self.inner.put_cf(cf, path.to_string(), bytes)?;
        Ok(())
    }
}
