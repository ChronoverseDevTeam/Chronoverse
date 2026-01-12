use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};


#[derive(Clone)]
pub struct LockedFile {
    /// depot path
    path: String,
    /// locked file revision, when file not exists, this should be empty
    locked_file_revision: String,
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
    locked_files: RwLock<HashSet<String>>,
    /// contexts of submitting
    contexts: RwLock<HashMap<uuid::Uuid, Arc<SubmitContext>>>
}

pub struct LaunchSubmitSuccess {
    ticket: String,
}

pub struct LaunchSubmitFailure {
    file_unable_to_lock: Vec<LockedFile>,
}

impl SubmitService {
    pub fn new() -> Self {
        Self {
            locked_files: RwLock::new(HashSet::new()),
            contexts: RwLock::new(HashMap::new()),
        }
    }

    pub async fn launch_submit(
        &mut self,
        files: &Vec<LockedFile>,
        submitting_by: String,
        timeout: chrono::Duration,
    ) -> Result<LaunchSubmitSuccess, LaunchSubmitFailure> {
        // 1) 先检查是否有任何文件已被锁定（全有或全无，不做部分加锁）
        let unique_paths: HashSet<String> = files.into_iter().map(|f| (&f.path).clone()).collect();
        {
            let mut locked = self
                .locked_files
                .write()
                .expect("submit service locked_files poisoned");

            let mut conflicted = Vec::new();
            for p in &unique_paths {
                if locked.contains(p) {
                    conflicted.push(LockedFile {
                        path: p.clone(),
                        locked_file_revision: String::new(),
                    });
                }
            }

            if !conflicted.is_empty() {
                return Err(LaunchSubmitFailure {
                    file_unable_to_lock: conflicted,
                });
            }

            for p in &unique_paths {
                locked.insert(p.clone());
            }
        }

        let ticket = uuid::Uuid::new_v4();
        let deadline = chrono::Utc::now() + timeout;

        // 2) 写入上下文
        {
            let ctx = Arc::new(SubmitContext {
                ticket,
                submitting_by,
                timeout_deadline: deadline,
                files: files.clone(),
            });
            let mut contexts = self
                .contexts
                .write()
                .expect("submit service contexts poisoned");
            contexts.insert(ticket, ctx);
        }

        Ok(LaunchSubmitSuccess {
            ticket: ticket.to_string(),
        })
    }
}