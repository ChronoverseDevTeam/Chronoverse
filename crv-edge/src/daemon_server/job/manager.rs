use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use super::core::{Job, JobId, MessageStoragePolicy};

pub struct JobManager {
    jobs: RwLock<HashMap<JobId, Arc<Job>>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
        }
    }

    pub fn create_job(&self, request_payload: Option<String>, buffer_policy: MessageStoragePolicy) -> Arc<Job> {
        let id = Uuid::new_v4().to_string();
        let job = Arc::new(Job::new(id.clone(), request_payload, buffer_policy));
        self.jobs.write().unwrap().insert(id, job.clone());
        job
    }

    pub fn get_job(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.read().unwrap().get(id).cloned()
    }
    
    pub fn remove_job(&self, id: &str) {
        self.jobs.write().unwrap().remove(id);
    }
}