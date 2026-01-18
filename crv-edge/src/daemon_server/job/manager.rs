use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use tokio::sync::mpsc;
use super::core::{Job, JobId, MessageStoragePolicy, WorkerProtocol};

pub struct JobManager {
    jobs: Arc<RwLock<HashMap<JobId, Arc<Job>>>>,
    cleanup_tx: mpsc::UnboundedSender<String>,
}

impl JobManager {
    pub fn new() -> Self {
        let jobs = Arc::new(RwLock::new(HashMap::new()));
        let jobs_clone = jobs.clone();
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        tokio::spawn(async move {
            while let Some(id) = rx.recv().await {
                if jobs_clone.write().unwrap().remove(&id).is_some() {
                    println!("[JobManager] Auto-cleaned job: {}", id);
                }
            }
        });

        Self {
            jobs,
            cleanup_tx: tx,
        }
    }

    pub fn create_job(&self, request_payload: Option<String>, buffer_policy: MessageStoragePolicy, protocol: WorkerProtocol) -> Arc<Job> {
        let id = Uuid::new_v4().to_string();
        println!("[JobManager] Creating job: {}", id);
        let job = Arc::new(Job::new(id.clone(), request_payload, buffer_policy, protocol, self.cleanup_tx.clone()));
        self.jobs.write().unwrap().insert(id.clone(), job.clone());
        job
    }

    pub fn get_job(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.read().unwrap().get(id).cloned()
    }
    
    pub fn remove_job(&self, id: &str) {
        if self.jobs.write().unwrap().remove(id).is_some() {
            println!("[JobManager] Removed job: {}", id);
        } else {
            println!("[JobManager] Attempted to remove non-existent job: {}", id);
        }
    }
}