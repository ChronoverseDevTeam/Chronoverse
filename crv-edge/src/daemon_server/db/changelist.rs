use crate::daemon_server::db::*;
use bincode::{Decode, Encode};
use crv_core::path::basic::WorkspacePath;

#[derive(Encode, Decode)]
pub struct ChangelistMeta {
    description: String,
    workspace_name: String,
    workspace_paths: Vec<WorkspacePath>,
}

impl DbManager {
    const KEY_CHANGELIST_COUNTER: &'static str = "changelist-number-counter";

    /// Create a changelist and return the changelist id.
    ///
    /// Currently, changelist id is the changlist counter when a changelist is created.
    /// Changelist id is parse as u64 currently.
    ///
    /// If the workspace is unexists, the changelist will still be created
    /// and become an orphan changelist.
    /// So in the future there should be a method to clear orphan changelists.
    pub fn create_changelist(
        &mut self,
        description: String,
        workspace_name: String,
    ) -> Result<String, DbError> {
        let new_value = bincode::encode_to_vec(
            ChangelistMeta {
                description,
                workspace_name,
                workspace_paths: Vec::new(),
            },
            bincode::config::standard(),
        )?;

        let next_changelist_counter = loop {
            let transaction = self.inner.transaction();
            let changelist_cf = self
                .inner
                .cf_handle(Self::CF_CHANGELIST)
                .expect(&format!("cf {} must exist", Self::CF_CHANGELIST));

            let changelist_counter =
                transaction.get_cf(changelist_cf, Self::KEY_CHANGELIST_COUNTER)?;

            let changelist_counter = match changelist_counter {
                Some(counter_bytes) => String::from_utf8_lossy(&counter_bytes)
                    .to_string()
                    .parse::<u64>()
                    .expect("Bad changelist counter."),
                None => 0u64,
            };

            let next_changelist_counter = changelist_counter + 1;

            transaction.put_cf(
                changelist_cf,
                Self::KEY_CHANGELIST_COUNTER,
                format!("{}", next_changelist_counter),
            )?;

            transaction.put_cf(
                changelist_cf,
                &format!("{}", next_changelist_counter),
                &new_value,
            )?;

            if transaction.commit().is_ok() {
                break next_changelist_counter;
            }
        };

        Ok(format!("{}", next_changelist_counter))
    }

    pub fn delete_changelist(&mut self, changelist_id: &String) -> Result<(), DbError> {
        let changelist_cf = self
            .inner
            .cf_handle(Self::CF_CHANGELIST)
            .expect(&format!("cf {} must exist", Self::CF_CHANGELIST));
        self.inner.delete_cf(changelist_cf, changelist_id)?;
        Ok(())
    }

    /// Append workspace paths to changelist.
    ///
    /// If any workspace path is not under the workspace of the changelist, return DbError::Invalid.
    pub fn append_changelist_workspace_paths(
        &mut self,
        changelist_id: &String,
        workspace_paths: Vec<WorkspacePath>,
    ) -> Result<(), DbError> {
        loop {
            let transaction = self.inner.transaction();
            let changelist_cf = self
                .inner
                .cf_handle(Self::CF_CHANGELIST)
                .expect(&format!("cf {} must exist", Self::CF_CHANGELIST));

            let mut changelist_meta: ChangelistMeta = match transaction
                .get_cf(changelist_cf, changelist_id)?
            {
                Some(bytes) => bincode::decode_from_slice(&bytes, bincode::config::standard())?.0,
                None => {
                    return Err(DbError::NotFound(format!(
                        "Changelist {changelist_id} does not exist."
                    )));
                }
            };

            for path in &workspace_paths {
                if path.workspace_name != changelist_meta.workspace_name {
                    return Err(DbError::Invalid(format!(
                        "Workspace path {} not under the workspace of changelist {}",
                        path.to_string(),
                        changelist_id
                    )));
                }
                changelist_meta.workspace_paths.push(path.clone());
            }

            transaction.put_cf(
                changelist_cf,
                changelist_id,
                bincode::encode_to_vec(changelist_meta, bincode::config::standard())?,
            )?;

            if transaction.commit().is_ok() {
                break;
            }
        }

        Ok(())
    }

    /// This method will iter through all local changelists,
    /// which may be slow when there are a lot of local changelists.
    pub fn get_changelist_id_by_workspace(
        &mut self,
        workspace_name: &String,
    ) -> Result<Vec<String>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_CHANGELIST)
            .expect(&format!("cf {} must exist", Self::CF_CHANGELIST));

        let iterator = self.inner.iterator_cf(cf, IteratorMode::Start);

        let mut result = Vec::new();
        for item in iterator {
            let (key, value) = item?;
            let meta: ChangelistMeta =
                bincode::decode_from_slice(&value, bincode::config::standard())?.0;
            if &meta.workspace_name == workspace_name {
                result.push(String::from_utf8_lossy(&key).to_string());
            }
        }

        Ok(result)
    }

    pub fn get_changelist_meta(
        &mut self,
        changelist_id: &String,
    ) -> Result<Option<ChangelistMeta>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_CHANGELIST)
            .expect(&format!("cf {} must exist", Self::CF_CHANGELIST));
        match self.inner.get_cf(cf, changelist_id)? {
            Some(bytes) => {
                let meta: ChangelistMeta =
                    bincode::decode_from_slice(&bytes, bincode::config::standard())?.0;
                Ok(Some(meta))
            }
            None => Ok(None),
        }
    }
}
