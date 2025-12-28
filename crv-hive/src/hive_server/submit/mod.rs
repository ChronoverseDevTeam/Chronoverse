use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::{Arc, OnceLock, RwLock},
};

pub mod launch_submit;
pub mod submit;

use chrono::{Duration, Utc};
use uuid::Uuid;

pub use self::submit::UploadFileChunkStream;

pub trait SubmitManager {
    fn create_ticket(&self, timeout: Duration) -> String;
    /// 刷新 ticket 的过期时间（续租）。
    ///
    /// 用于支持“短 TTL + 活跃续租”的策略：客户端在每次有效操作（如上传 chunk / 提交）前
    /// 先 touch 一次 ticket，把过期时间延后，避免长时间占用锁。
    fn touch_ticket(&self, ticket: &str, timeout: Duration) -> Result<(), Box<dyn Error>>;
    fn lock_file(&self, branch_id: &str, file_path: &str, ticket: String) -> Result<Arc<String>, Box<dyn Error>>;
    fn batch_lock_files(&self, branch_id: &str, files: &Vec<String>, ticket: String) -> Result<Vec<Arc<String>>, Box<dyn Error>>;
    fn unlock_file(&self, branch_id: &str, file_path: &str) -> Result<(), Box<dyn Error>>;
    fn check_file_locked(&self, branch_id: &str, file_path: &str) -> Result<String, Box<dyn Error>>;
}

pub struct SubmitManagerImpl {
    /// 记录 ticket 过期时间的 API，一旦 ticket 到达过期时间点，就触发 ticket 的清理逻辑，自动标记 ticket 为提交失败，解锁所有已锁定的文件
    ticket_records: RwLock<HashMap<String, chrono::DateTime<Utc>>>,
    // 锁结构： Map<branch_id, Map<file_path, ticket>>
    lock_records: RwLock<HashMap<String, HashMap<String, Arc<String>>>>,
}

impl SubmitManagerImpl {
    pub fn new() -> Self {
        Self {
            ticket_records: RwLock::new(HashMap::new()),
            lock_records: RwLock::new(HashMap::new()),
        }
    }

    fn cleanup_expired_tickets(&self) -> Result<usize, Box<dyn Error>> {
        let now = Utc::now();

        // 1) 移除过期 ticket，并收集过期列表
        let expired: Vec<String> = {
            let mut tickets = self
                .ticket_records
                .write()
                .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

            let expired: Vec<String> = tickets
                .iter()
                .filter_map(|(ticket, expires_at)| {
                    if *expires_at <= now {
                        Some(ticket.clone())
                    } else {
                        None
                    }
                })
                .collect();

            for t in &expired {
                tickets.remove(t);
            }

            expired
        };

        if expired.is_empty() {
            return Ok(0);
        }

        // 2) 清理所有被这些 ticket 锁定的文件
        let expired_set: HashSet<String> = expired.into_iter().collect();
        let mut records = self
            .lock_records
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

        let mut empty_branches = Vec::new();
        for (branch_id, branch_locks) in records.iter_mut() {
            branch_locks.retain(|_path, ticket_arc| !expired_set.contains(ticket_arc.as_str()));
            if branch_locks.is_empty() {
                empty_branches.push(branch_id.clone());
            }
        }
        for b in empty_branches {
            records.remove(&b);
        }

        Ok(expired_set.len())
    }

    fn ensure_ticket_active(&self, ticket: &str) -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        let tickets = self
            .ticket_records
            .read()
            .map_err(|e| format!("Failed to acquire read lock: {}", e))?;

        let expires_at = tickets
            .get(ticket)
            .ok_or_else(|| format!("Invalid ticket: {}", ticket))?;

        if *expires_at <= now {
            return Err(format!("Ticket expired: {}", ticket).into());
        }

        Ok(())
    }

    pub fn cancel_ticket(&self, ticket: &str) -> Result<(), Box<dyn Error>> {
        // 先清理过期票据，避免 cancel 时遗漏已过期的锁。
        let _ = self.cleanup_expired_tickets();

        // 删除 ticket 记录
        {
            let mut tickets = self
                .ticket_records
                .write()
                .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
            tickets.remove(ticket);
        }

        // 移除所有由该 ticket 产生的锁
        let mut records = self
            .lock_records
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

        let mut empty_branches = Vec::new();
        for (branch_id, branch_locks) in records.iter_mut() {
            branch_locks.retain(|_path, ticket_arc| ticket_arc.as_str() != ticket);
            if branch_locks.is_empty() {
                empty_branches.push(branch_id.clone());
            }
        }
        for b in empty_branches {
            records.remove(&b);
        }

        Ok(())
    }
}

impl SubmitManager for SubmitManagerImpl {
    fn create_ticket(&self, timeout: Duration) -> String {
        // 创建 ticket 并记录过期时间
        let ticket = Uuid::new_v4().to_string().replace('-', "");
        let expires_at = Utc::now() + timeout;
        if let Ok(mut tickets) = self.ticket_records.write() {
            tickets.insert(ticket.clone(), expires_at);
        }
        ticket
    }

    fn touch_ticket(&self, ticket: &str, timeout: Duration) -> Result<(), Box<dyn Error>> {
        // 先清掉过期票据，避免刷新到已经过期且应被回收的 ticket
        let _ = self.cleanup_expired_tickets();

        // 只有活跃 ticket 才允许续租
        self.ensure_ticket_active(ticket)?;

        let expires_at = Utc::now() + timeout;
        let mut tickets = self
            .ticket_records
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

        if !tickets.contains_key(ticket) {
            return Err(format!("Invalid ticket: {}", ticket).into());
        }

        tickets.insert(ticket.to_string(), expires_at);
        Ok(())
    }

    fn lock_file(&self, branch_id: &str, file_path: &str, ticket: String) -> Result<Arc<String>, Box<dyn Error>> {
        // 在每次写操作前做一次惰性清理，避免过期 ticket 长期占用锁
        let _ = self.cleanup_expired_tickets();
        self.ensure_ticket_active(&ticket)?;

        let mut records = self.lock_records.write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
        
        // 获取或创建分支的锁记录
        let branch_locks = records.entry(branch_id.to_string())
            .or_insert_with(HashMap::new);
        
        // 检查文件是否已被锁定
        if let Some(existing_ticket) = branch_locks.get(file_path) {
            return Err(format!(
                "File '{}' in branch '{}' is already locked by ticket '{}'",
                file_path,
                branch_id,
                existing_ticket
            ).into());
        }
        
        // 锁定文件，存储 ticket
        let ticket_arc = Arc::new(ticket);
        branch_locks.insert(file_path.to_string(), ticket_arc.clone());
        
        Ok(ticket_arc)
    }
    
    fn batch_lock_files(&self, branch_id: &str, files: &Vec<String>, ticket: String) -> Result<Vec<Arc<String>>, Box<dyn Error>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }

        let _ = self.cleanup_expired_tickets();
        self.ensure_ticket_active(&ticket)?;
        
        let mut records = self.lock_records.write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
        
        // 获取或创建分支的锁记录
        let branch_locks = records.entry(branch_id.to_string())
            .or_insert_with(HashMap::new);
        
        // 先检查所有文件是否都可用（未被锁定）
        let mut conflicted_files = Vec::new();
        for file_path in files {
            if let Some(existing_ticket) = branch_locks.get(file_path) {
                conflicted_files.push((file_path.clone(), existing_ticket.as_str().to_string()));
            }
        }
        
        // 如果有冲突，返回错误
        if !conflicted_files.is_empty() {
            let conflicts: Vec<String> = conflicted_files
                .iter()
                .map(|(path, ticket)| format!("'{}' (locked by ticket '{}')", path, ticket))
                .collect();
            return Err(format!(
                "Cannot lock files in branch '{}': {}",
                branch_id,
                conflicts.join(", ")
            ).into());
        }
        
        // 所有文件都可用，批量锁定
        let ticket_arc = Arc::new(ticket);
        let mut result = Vec::with_capacity(files.len());
        
        for file_path in files {
            branch_locks.insert(file_path.clone(), ticket_arc.clone());
            result.push(ticket_arc.clone());
        }
        
        Ok(result)
    }
    
    fn unlock_file(&self, branch_id: &str, file_path: &str) -> Result<(), Box<dyn Error>> {
        let _ = self.cleanup_expired_tickets();

        let mut records = self.lock_records.write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
        
        // 获取分支的锁记录
        let branch_locks = records.get_mut(branch_id)
            .ok_or_else(|| format!("Branch '{}' has no lock records", branch_id))?;
        
        // 检查文件是否被锁定
        if branch_locks.remove(file_path).is_none() {
            return Err(format!("File '{}' in branch '{}' is not locked", file_path, branch_id).into());
        }
        
        // 如果分支下没有锁定的文件了，清理空的分支记录
        if branch_locks.is_empty() {
            records.remove(branch_id);
        }
        
        Ok(())
    }
    
    fn check_file_locked(&self, branch_id: &str, file_path: &str) -> Result<String, Box<dyn Error>> {
        let _ = self.cleanup_expired_tickets();

        let records = self.lock_records.read()
            .map_err(|e| format!("Failed to acquire read lock: {}", e))?;
        
        // 获取分支的锁记录
        let branch_locks = records.get(branch_id)
            .ok_or_else(|| format!("Branch '{}' has no lock records", branch_id))?;
        
        match branch_locks.get(file_path) {
            Some(ticket_arc) => Ok(ticket_arc.as_str().to_string()),
            None => Err(format!("File '{}' in branch '{}' is not locked", file_path, branch_id).into()),
        }
    }
}

impl Default for SubmitManagerImpl {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局 SubmitManager 实例，用于管理文件锁定。
static SUBMIT_MANAGER: OnceLock<Arc<SubmitManagerImpl>> = OnceLock::new();
static SUBMIT_TICKET_CLEANER_STARTED: OnceLock<()> = OnceLock::new();

pub(crate) fn submit_manager() -> &'static Arc<SubmitManagerImpl> {
    let mgr = SUBMIT_MANAGER.get_or_init(|| Arc::new(SubmitManagerImpl::new()));

    // 启动后台清理任务：定期移除过期 ticket，并释放其持有的文件锁
    SUBMIT_TICKET_CLEANER_STARTED.get_or_init(|| {
        let mgr = Arc::clone(mgr);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let _ = mgr.cleanup_expired_tickets();
            }
        });
    });

    mgr
}

struct LockedFile {
    file_id: String,
    path: String,
    
}

struct  SubmitContext {
    ticket: String,
    files: Vec<LockedFile>,
}

