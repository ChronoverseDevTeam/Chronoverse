use futures::future::BoxFuture;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Notify, broadcast, mpsc};
use tokio::task::JoinSet;

pub type JobId = String;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WorkerProtocol {
    /// All workers must succeed. If any fails, the job fails immediately.
    And,
    /// Job fails only if ALL workers fail; succeeds if any worker completes.
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum JobRetentionPolicy {
    /// Immediately remove job when finished.
    Immediate,
    /// Retain job for specified seconds after finished.
    Retain(u64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
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
    retention_policy: JobRetentionPolicy,
    cancel_notify: Arc<Notify>,
}

impl Job {
    pub fn new(
        id: JobId,
        request_payload: Option<String>,
        buffer_policy: MessageStoragePolicy,
        protocol: WorkerProtocol,
        retention_policy: JobRetentionPolicy,
        cleanup_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        let (tx, _) = broadcast::channel(1024);
        let now = current_timestamp();

        let (message_buffer, buffer_capacity) = match buffer_policy {
            MessageStoragePolicy::None => (None, 0),
            MessageStoragePolicy::RingBuffer(cap) => {
                (Some(Mutex::new(VecDeque::with_capacity(cap))), cap)
            }
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
            retention_policy,
            cancel_notify: Arc::new(Notify::new()),
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
        data.updated_at = current_timestamp();
        drop(data);

        let workers = std::mem::take(&mut *self.pending_workers.lock().unwrap());
        let protocol = self.protocol;
        let job_ref = self.clone();
        let cancel_notify = self.cancel_notify.clone();

        tokio::spawn(async move {
            let mut set = JoinSet::new();
            let total_workers = workers.len();
            for w in workers {
                set.spawn(w);
            }

            let mut failures = 0;

            loop {
                tokio::select! {
                    _ = cancel_notify.notified() => {
                        set.abort_all();
                        // Status update is handled in cancel()
                        return;
                    }
                    res = set.join_next() => {
                        match res {
                            Some(result) => {
                                match protocol {
                                    WorkerProtocol::And => {
                                        match result {
                                            Ok(Ok(_)) => {},
                                            Ok(Err(e)) => {
                                                job_ref.fail(format!("Worker failed: {}", e));
                                                set.abort_all();
                                                return;
                                            }
                                            Err(e) => {
                                                job_ref.fail(format!("Worker panic: {}", e));
                                                set.abort_all();
                                                return;
                                            }
                                        }
                                    },
                                    WorkerProtocol::Or => {
                                        match result {
                                            Ok(Ok(_)) => {},
                                            Ok(Err(_)) => failures += 1,
                                            Err(_) => failures += 1,
                                        }
                                    }
                                }
                            },
                            None => {
                                // All workers finished
                                match protocol {
                                    WorkerProtocol::And => {
                                        job_ref.complete();
                                    },
                                    WorkerProtocol::Or => {
                                        if failures == total_workers && total_workers > 0 {
                                            job_ref.fail("All workers failed".to_string());
                                        } else {
                                            job_ref.complete();
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    pub fn cancel(&self) {
        if self.transition_to_terminal(JobStatus::Cancelled) {
            self.broadcast(JobEvent::StatusChange(JobStatus::Cancelled));
            self.cancel_notify.notify_waiters();
            self.trigger_cleanup();
        }
    }

    pub fn complete(&self) {
        if self.transition_to_terminal(JobStatus::Completed) {
            self.broadcast(JobEvent::StatusChange(JobStatus::Completed));
            self.trigger_cleanup();
        }
    }

    pub fn fail(&self, error: String) {
        if self.transition_to_terminal(JobStatus::Failed(error.clone())) {
            self.broadcast(JobEvent::Error(error.clone()));
            self.broadcast(JobEvent::StatusChange(JobStatus::Failed(error)));
            self.trigger_cleanup();
        }
    }

    fn transition_to_terminal(&self, new_status: JobStatus) -> bool {
        let mut data = self.data.write().unwrap();
        if matches!(
            data.status,
            JobStatus::Completed | JobStatus::Failed(_) | JobStatus::Cancelled
        ) {
            return false;
        }
        data.status = new_status;
        data.updated_at = current_timestamp();
        true
    }

    fn trigger_cleanup(&self) {
        let tx = self.cleanup_tx.clone();
        let id = self.data.read().unwrap().id.clone();
        let policy = self.retention_policy;

        match policy {
            JobRetentionPolicy::Immediate => {
                let _ = tx.send(id);
            }
            JobRetentionPolicy::Retain(secs) => {
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
                    let _ = tx.send(id);
                });
            }
        }
    }

    pub fn report_payload<T: Message>(&self, msg: T) {
        let mut value = Vec::new();
        msg.encode(&mut value).unwrap();
        let type_url = std::any::type_name::<T>().to_string();
        let any = prost_types::Any { type_url, value };
        self.broadcast(JobEvent::Payload(any));
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
