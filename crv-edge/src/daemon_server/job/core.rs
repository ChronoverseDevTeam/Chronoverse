use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinSet;
use futures::future::BoxFuture;
use prost::Message;

pub type JobId = String;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WorkerProtocol {
    /// All workers must succeed. If any fails, the job fails immediately.
    And,
    /// The job succeeds if the completion condition is met (e.g., all workers finished, regardless of success/failure state, 
    /// but usually 'Or' implies distinct logic. Here we define: Job fails only if ALL workers fail).
    /// For this implementation: We track failures. If failure_count == total_workers, Job Fails.
    /// If any worker succeeds, the Job eventually Succeeds (once all are done).
    Or,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    StatusChange(JobStatus),
    Error(String),
    Payload(prost_types::Any),
}

/// Job 的持久化数据部分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobData {
    pub id: JobId,
    pub status: JobStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub request_payload: Option<String>, 
}

/// 消息存储策略
pub enum MessageStoragePolicy {
    None,
    RingBuffer(usize),
}

/// Job 的运行时部分
pub struct Job {
    pub data: RwLock<JobData>,
    pub tx: broadcast::Sender<JobEvent>,
    pub message_buffer: Option<Mutex<VecDeque<JobEvent>>>,
    pub buffer_capacity: usize,
    
    // Worker management
    pending_workers: Mutex<Vec<BoxFuture<'static, Result<(), String>>>>,
    protocol: WorkerProtocol,
    cleanup_tx: mpsc::UnboundedSender<String>,
}

impl Job {
    pub fn new(
        id: JobId, 
        request_payload: Option<String>, 
        buffer_policy: MessageStoragePolicy,
        protocol: WorkerProtocol,
        cleanup_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        let (tx, _) = broadcast::channel(1024);
        let now = current_timestamp();
        
        let (message_buffer, buffer_capacity) = match buffer_policy {
            MessageStoragePolicy::None => (None, 0),
            MessageStoragePolicy::RingBuffer(cap) => (Some(Mutex::new(VecDeque::with_capacity(cap))), cap),
        };

        Self {
            data: RwLock::new(JobData {
                id,
                status: JobStatus::Pending,
                created_at: now,
                updated_at: now,
                request_payload,
            }),
            tx,
            message_buffer,
            buffer_capacity,
            pending_workers: Mutex::new(Vec::new()),
            protocol,
            cleanup_tx,
        }
    }

    pub fn add_worker<F>(&self, future: F)
    where
        F: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        self.pending_workers.lock().unwrap().push(Box::pin(future));
    }

    pub fn start(self: Arc<Self>) {
        let mut data = self.data.write().unwrap();
        if data.status != JobStatus::Pending {
            return;
        }
        data.status = JobStatus::Running;
        drop(data);

        let workers = std::mem::take(&mut *self.pending_workers.lock().unwrap());
        let protocol = self.protocol;
        let job_ref = self.clone();

        tokio::spawn(async move {
            let mut set = JoinSet::new();
            let total_workers = workers.len();
            for w in workers {
                set.spawn(w);
            }

            let mut failures = 0;

            match protocol {
                WorkerProtocol::And => {
                    while let Some(res) = set.join_next().await {
                        match res {
                            Ok(Ok(_)) => {
                                // Success
                            }
                            Ok(Err(e)) => {
                                job_ref.fail(format!("Worker failed: {}", e));
                                set.abort_all();
                                return; // Fail immediately
                            }
                            Err(e) => {
                                job_ref.fail(format!("Worker panic: {}", e));
                                set.abort_all();
                                return;
                            }
                        }
                    }
                    // If we get here, all succeeded
                    job_ref.complete();
                }
                WorkerProtocol::Or => {
                    while let Some(res) = set.join_next().await {
                        match res {
                            Ok(Ok(_)) => {},
                            Ok(Err(_)) => failures += 1, // Log error?
                            Err(_) => failures += 1,
                        }
                    }
                    if failures == total_workers && total_workers > 0 {
                        job_ref.fail("All workers failed".to_string());
                    } else {
                        job_ref.complete();
                    }
                }
            }
        });
    }

    pub fn complete(&self) {
        self.update_status(JobStatus::Completed);
        self.broadcast(JobEvent::StatusChange(JobStatus::Completed));
        let _ = self.cleanup_tx.send(self.data.read().unwrap().id.clone());
    }

    pub fn fail(&self, error: String) {
        self.update_status(JobStatus::Failed(error.clone()));
        self.broadcast(JobEvent::Error(error.clone()));
        self.broadcast(JobEvent::StatusChange(JobStatus::Failed(error)));
        let _ = self.cleanup_tx.send(self.data.read().unwrap().id.clone());
    }

    pub fn report_payload<T: Message>(&self, msg: T) {
         let mut value = Vec::new();
         msg.encode(&mut value).unwrap();
         let type_url = std::any::type_name::<T>().to_string();
         let any = prost_types::Any {
             type_url,
             value,
         };
         self.broadcast(JobEvent::Payload(any));
    }

    fn update_status(&self, status: JobStatus) {
        let mut data = self.data.write().unwrap();
        data.status = status;
        data.updated_at = current_timestamp();
    }
    
    fn broadcast(&self, event: JobEvent) {
        // Save to buffer if policy enabled
        if let Some(ref buffer) = self.message_buffer {
            let mut buf = buffer.lock().unwrap();
            if buf.len() >= self.buffer_capacity && self.buffer_capacity > 0 {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }
        let _ = self.tx.send(event);
    }

    pub fn consume_buffered_events(&self) -> Vec<JobEvent> {
        if let Some(ref buffer) = self.message_buffer {
            let mut buf = buffer.lock().unwrap();
            buf.drain(..).collect()
        } else {
            Vec::new()
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}