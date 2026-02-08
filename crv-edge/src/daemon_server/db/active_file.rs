//! Active file 也就是可以被提交的 file，亦即 checkout 的 file

use crate::daemon_server::db::*;
use bincode::{Decode, Encode};
use crv_core::path::basic::{WorkspaceDir, WorkspacePath};

#[derive(Encode, Decode, PartialEq, Eq, Clone)]
pub enum Action {
    Add,
    Delete,
    Edit,
}

impl Action {
    pub fn to_custom_string(&self) -> String {
        match self {
            Action::Add => "add".to_string(),
            Action::Edit => "edit".to_string(),
            Action::Delete => "delete".to_string(),
        }
    }
}

impl DbManager {
    pub fn set_active_file_action(
        &self,
        path: WorkspacePath,
        action: Action,
    ) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_ACTIVE_FILE)
            .expect(&format!("cf {} must exist", Self::CF_ACTIVE_FILE));
        let bytes = bincode::encode_to_vec(action, bincode::config::standard())?;
        self.inner.put_cf(cf, path.to_custom_string(), bytes)?;
        Ok(())
    }

    pub fn get_active_file_action(&self, path: &WorkspacePath) -> Result<Option<Action>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_ACTIVE_FILE)
            .expect(&format!("cf {} must exist", Self::CF_ACTIVE_FILE));
        match self.inner.get_cf(cf, path.to_custom_string())? {
            Some(bytes) => {
                let action: Action =
                    bincode::decode_from_slice(&bytes, bincode::config::standard())?.0;
                Ok(Some(action))
            }
            None => Ok(None),
        }
    }

    pub fn get_active_file_under_dir(
        &self,
        dir: &WorkspaceDir,
    ) -> Result<Vec<(WorkspacePath, Action)>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_ACTIVE_FILE)
            .expect(&format!("cf {} must exist", Self::CF_ACTIVE_FILE));
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
                    let action: Action =
                        bincode::decode_from_slice(&value, bincode::config::standard())?.0;
                    result.push((workspace_path, action));
                }
                Err(e) => return Err(DbError::RocksDb(e)),
            }
        }

        return Ok(result);
    }
}
