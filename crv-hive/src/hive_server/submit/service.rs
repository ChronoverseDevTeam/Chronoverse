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

    pub fn launch_submit(
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

        // 1) 先检查是否有任何文件已被锁定（全有或全无，不做部分加锁）
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
                    .map(|p| {
                        files.iter().find(|f| f.path == p).unwrap().clone()
                    })
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
                    conflicted.push(
                        files.iter().find(|f| f.path == *p).unwrap().clone()
                    );
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

        Ok(LaunchSubmitSuccess {
            ticket: ticket,
        })
    }

    pub fn submit(&mut self, ticket: &uuid::Uuid) {
        let mut context = self.contexts.write().expect("submit service contexts poisoned");

        let context = context.get(ticket);
        if context.is_none() {
        }
    }
}
