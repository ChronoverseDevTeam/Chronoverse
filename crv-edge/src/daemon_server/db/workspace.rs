use crate::daemon_server::db::*;
use bincode::{Decode, Encode};
use crv_core::{path::basic::LocalDir, workspace::entity::WorkspaceMapping};

#[derive(Encode, Decode)]
pub struct WorkspaceMeta {
    status: Status,
    root_dir: LocalDir,
    mapping_views: WorkspaceMapping,
}

impl DbManager {
    const KEY_WORKSPACE_META_REVISON: &'static str = "workspace";

    /// 这个方法用于创建一个 workspace，它会检查预创建的 workspace 的 root path 是否和
    /// 某个已有的 worksapce 的 root path 相冲突，但是不会检查 mapping views 是否合法，
    /// 因为 mapping views 是否合法与其他 workspace 无关
    ///
    /// 这个方法创建出来的数据处于 Pending 状态，需要转化为 Confirmed 才能投入使用
    pub fn create_workspace_pending(
        &mut self,
        workspace_name: String,
        root_dir: LocalDir,
        mapping_views: WorkspaceMapping,
    ) -> Result<(), DbError> {
        let new_value = bincode::encode_to_vec(
            WorkspaceMeta {
                status: Status::Pending,
                root_dir: root_dir.clone(),
                mapping_views,
            },
            bincode::config::standard(),
        )?;
        loop {
            // step 1. 检查 root dir 是否和某个 workspace 冲突
            let transaction = self.inner.transaction();
            let workspace_cf = self
                .inner
                .cf_handle(Self::CF_WORKSPACE)
                .expect(&format!("cf {} must exist", Self::CF_WORKSPACE));
            let iter = transaction.iterator_cf(workspace_cf, IteratorMode::Start);
            let mut conflict_workspace = Vec::new();

            for item in iter {
                match item {
                    Ok((key, value)) => {
                        let exist_workspace_name = String::from_utf8_lossy(&key).to_string();
                        let meta: WorkspaceMeta =
                            bincode::decode_from_slice(&value, bincode::config::standard())?.0;
                        // 新 workspace 不能和本地已有的 workspace 命名冲突
                        if workspace_name == exist_workspace_name {
                            return Err(DbError::WorkspaceConflict(format!(
                                "{} already exists.",
                                workspace_name
                            )));
                        }
                        // 新 workspace 不能和本地已有的 workspace 根目录冲突
                        if root_dir.0.starts_with(&meta.root_dir.0)
                            || meta.root_dir.0.starts_with(&root_dir.0)
                        {
                            conflict_workspace.push(exist_workspace_name);
                        }
                    }
                    Err(e) => return Err(DbError::RocksDb(e)),
                }
            }

            if !conflict_workspace.is_empty() {
                let conflict_workspace_text = conflict_workspace.join(", ");
                return Err(DbError::WorkspaceConflict(format!(
                    "Workspace root conflicts with {}",
                    conflict_workspace_text
                )));
            }

            // step 2. 写入新 workspace
            transaction.put_cf(workspace_cf, &workspace_name, &new_value)?;

            // step 3. 写入 META_REVISION
            let meta_revision_cf = self
                .inner
                .cf_handle(Self::CF_META_REVISION)
                .expect(&format!("cf {} must exist", Self::CF_META_REVISION));
            let revision_id = uuid::Uuid::new_v4();
            transaction.put_cf(
                meta_revision_cf,
                Self::KEY_WORKSPACE_META_REVISON,
                revision_id.as_bytes(),
            )?;

            // step 4. 提交事务
            if transaction.commit().is_ok() {
                break;
            }
        }

        Ok(())
    }

    pub fn confirm_workspace(&mut self, workspace_name: String) -> Result<(), DbError> {
        let workspace_meta = self.get_workspace_meta(&workspace_name)?;
        if workspace_meta.is_none() {
            return Err(DbError::NotFound(format!(
                "{workspace_name} does not exist."
            )));
        }
        let mut workspace_meta = workspace_meta.unwrap();
        let cf = self
            .inner
            .cf_handle(Self::CF_WORKSPACE)
            .expect(&format!("cf {} must exist", Self::CF_WORKSPACE));
        workspace_meta.status = Status::Confirmed;

        self.inner.put_cf(
            cf,
            workspace_name,
            &bincode::encode_to_vec(workspace_meta, bincode::config::standard())?,
        )?;
        Ok(())
    }

    pub fn get_workspace_meta(
        &mut self,
        workspace_name: &String,
    ) -> Result<Option<WorkspaceMeta>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_WORKSPACE)
            .expect(&format!("cf {} must exist", Self::CF_WORKSPACE));
        match self.inner.get_cf(cf, workspace_name)? {
            Some(bytes) => {
                let meta: WorkspaceMeta =
                    bincode::decode_from_slice(&bytes, bincode::config::standard())?.0;
                Ok(Some(meta))
            }
            None => Ok(None),
        }
    }

    pub fn get_workspace_name_by_working_dir(
        &mut self,
        dir: &LocalDir,
    ) -> Result<Option<String>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_WORKSPACE)
            .expect(&format!("cf {} must exist", Self::CF_WORKSPACE));
        let iter = self.inner.iterator_cf(cf, IteratorMode::Start);

        for item in iter {
            match item {
                Ok((key, value)) => {
                    let workspace_name = String::from_utf8_lossy(&key).to_string();
                    let meta: WorkspaceMeta =
                        bincode::decode_from_slice(&value, bincode::config::standard())?.0;
                    let root_dir = &meta.root_dir;
                    if dir.0.starts_with(&root_dir.0) {
                        return Ok(Some(workspace_name));
                    }
                }
                Err(e) => return Err(DbError::RocksDb(e)),
            }
        }

        return Ok(None);
    }
}
