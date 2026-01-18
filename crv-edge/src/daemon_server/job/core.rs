use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use crate::pb::{SyncProgress, TransferBlueprintRsp};

pub type JobId = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    Progress(SyncProgress),
    TransferBlueprint(TransferBlueprintRsp),
    StatusChange(JobStatus),
    Error(String),
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
}

impl Job {
    pub fn new(id: JobId, request_payload: Option<String>, buffer_policy: MessageStoragePolicy) -> Self {
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
        }
    }

    pub fn start(&self) {
        self.update_status(JobStatus::Running);
    }

    pub fn complete(&self) {
        self.update_status(JobStatus::Completed);
        self.broadcast(JobEvent::StatusChange(JobStatus::Completed));
    }

    pub fn fail(&self, error: String) {
        self.update_status(JobStatus::Failed(error.clone()));
        self.broadcast(JobEvent::Error(error.clone()));
        self.broadcast(JobEvent::StatusChange(JobStatus::Failed(error)));
    }

    pub fn report_progress(&self, progress: SyncProgress) {
        self.broadcast(JobEvent::Progress(progress));
    }
    
    pub fn report_transfer(&self, msg: TransferBlueprintRsp) {
        self.broadcast(JobEvent::TransferBlueprint(msg));
    }

    fn update_status(&self, status: JobStatus) {
        let mut data = self.data.write().unwrap();
        data.status = status;
        data.updated_at = current_timestamp();
    }
    
    fn broadcast(&self, event: JobEvent) {
        // 1. 发送给实时监听者
        let _ = self.tx.send(event.clone());
        
        // 2. 存入缓冲区 (如果开启)
        if let Some(ref buffer) = self.message_buffer {
            let mut buf = buffer.lock().unwrap();
            if buf.len() >= self.buffer_capacity && self.buffer_capacity > 0 {
                buf.pop_front();
            }
            buf.push_back(event);
        }
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
        .unwrap_or_default()
        .as_millis() as u64
}