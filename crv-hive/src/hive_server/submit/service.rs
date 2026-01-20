use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use crate::common::depot_path::DepotPath;

#[derive(Clone)]
pub struct LockedFile {
    /// depot path
    path: DepotPath,
    /// locked file generation, when file not exists, this should be None
    locked_generation: Option<i64>,
    /// locked file revision, when file not exists, this should be None
    locked_revision: Option<i64>,
}

#[derive(Clone)]
pub struct SubmittedFile {
    /// depot path
    path: DepotPath,
    /// chunk hashes
    chunk_hashs: Vec<String>,
}

pub struct SubmitContext {
    /// context's identity uuid
    ticket: uuid::Uuid,
    /// user that submitting this context
    submitting_by: String,
    /// this context will be removed after this deadline
    timeout_deadline: chrono::DateTime<chrono::Utc>,
    /// files that submitting
    files: Vec<LockedFile>,
}

pub struct SubmitService {
    /// locked files's paths
    locked_paths: RwLock<HashMap<DepotPath, uuid::Uuid>>,
    /// contexts of submitting
    contexts: RwLock<HashMap<uuid::Uuid, Arc<SubmitContext>>>,
}

pub struct LaunchSubmitSuccess {
    ticket: uuid::Uuid,
}

pub struct LaunchSubmitFailure {
    file_unable_to_lock: Vec<LockedFile>,
}

impl SubmitService {
    pub fn new() -> Self {
        Self {
            locked_paths: RwLock::new(HashMap::new()),
            contexts: RwLock::new(HashMap::new()),
        }
    }

    fn unlock_context(&self, ticket: &uuid::Uuid) {
        let mut locked = self
            .locked_paths
            .write()
            .expect("submit service locked_paths poisoned");
        let mut context = self
            .contexts
            .write()
            .expect("submit service contexts poisoned");

        let Some(ctx) = context.get(ticket) else {
            // already unlocked / unknown ticket
            return;
        };

        for f in ctx.files.iter() {
            // 只释放属于该 ticket 的锁，避免误删其他并发 ticket 的占用
            if locked.get(&f.path) == Some(ticket) {
                locked.remove(&f.path);
            }
        }
        context.remove(ticket);
    }

    pub async fn launch_submit(
        &mut self,
        files: &Vec<LockedFile>,
        submitting_by: String,
        timeout: chrono::Duration,
    ) -> Result<LaunchSubmitSuccess, LaunchSubmitFailure> {
        let ticket = uuid::Uuid::new_v4();

        if self
            .contexts
            .read()
            .expect("submit service contexts poisoned")
            .contains_key(&ticket)
        {
            return Err(LaunchSubmitFailure {
                file_unable_to_lock: Vec::new(),
            });
        }

        let deadline = chrono::Utc::now() + timeout;

        // 0) 去重
        let mut unique_paths: HashSet<DepotPath> = HashSet::new();
        let mut duplicated_paths: HashSet<DepotPath> = HashSet::new();
        for f in files.iter() {
            if !unique_paths.insert(f.path.clone()) {
                duplicated_paths.insert(f.path.clone());
            }
        }
        if !duplicated_paths.is_empty() {
            return Err(LaunchSubmitFailure {
                file_unable_to_lock: duplicated_paths
                    .into_iter()
                    .map(|p| files.iter().find(|f| f.path == p).unwrap().clone())
                    .collect(),
            });
        }

        {
            // 同时进行锁定，防止出现一致性问题
            let mut locked = self
                .locked_paths
                .write()
                .expect("submit service locked_paths poisoned");
            let mut contexts = self
                .contexts
                .write()
                .expect("submit service contexts poisoned");

            let mut conflicted = Vec::new();
            for p in &unique_paths {
                if locked.contains_key(p) {
                    conflicted.push(files.iter().find(|f| f.path == *p).unwrap().clone());
                }
            }

            if !conflicted.is_empty() {
                return Err(LaunchSubmitFailure {
                    file_unable_to_lock: conflicted,
                });
            }

            for p in &unique_paths {
                locked.insert(p.clone(), ticket);
            }

            // 2) 写入上下文
            let ctx = Arc::new(SubmitContext {
                ticket,
                submitting_by,
                timeout_deadline: deadline,
                files: files.clone(),
            });
            contexts.insert(ticket, ctx);
        }

        {
            // 2) 读数据库，对比最新版本是否和预期的锁定版本一致
            let mut conflicted = Vec::new();

            for p in &unique_paths {
                let f = files.iter().find(|f| f.path == *p).unwrap();

                let expected = match (f.locked_generation, f.locked_revision) {
                    (Some(g), Some(r)) => Some((g, r)),
                    (None, None) => None,
                    // generation/revision 只给了一个：视为非法期望版本
                    _ => {
                        conflicted.push(f.clone());
                        continue;
                    }
                };

                let current = match crate::database::dao::find_latest_file_revision_by_depot_path(
                    &p.to_string(),
                )
                .await
                {
                    Ok(latest) => latest.and_then(|m| {
                        if m.is_delete {
                            None
                        } else {
                            Some((m.generation, m.revision))
                        }
                    }),
                    Err(e) => {
                        // 约定：当期望版本为 None（即期望文件不存在）时，“查不到记录”属于预期内，视为 current=None
                        if expected.is_none()
                            && matches!(
                                e,
                                crate::database::dao::DaoError::Db(
                                    sea_orm::DbErr::RecordNotFound(_)
                                )
                            )
                        {
                            None
                        } else {
                            conflicted.push(f.clone());
                            continue;
                        }
                    }
                };

                if expected != current {
                    conflicted.push(f.clone());
                }
            }

            if !conflicted.is_empty() {
                // 回滚：释放本次 ticket 占用的锁与上下文
                self.unlock_context(&ticket);

                return Err(LaunchSubmitFailure {
                    file_unable_to_lock: conflicted,
                });
            }
        }

        Ok(LaunchSubmitSuccess { ticket: ticket })
    }

    pub fn submit(&mut self, ticket: &uuid::Uuid) {
        let mut context = self
            .contexts
            .write()
            .expect("submit service contexts poisoned");

        let context = context.get(ticket);
        if context.is_none() {}
    }
}
