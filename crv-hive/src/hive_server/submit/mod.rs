use std::{collections::HashMap, error::Error, sync::{Arc, OnceLock, RwLock}};

mod launch_submit;
mod submit;

pub use self::submit::UploadFileChunkStream;

pub trait SubmitManager {
    fn lock_file(&self, branch_id: &str, file_path: &str, ticket: String) -> Result<Arc<String>, Box<dyn Error>>;
    fn batch_lock_files(&self, branch_id: &str, files: &Vec<String>, ticket: String) -> Result<Vec<Arc<String>>, Box<dyn Error>>;
    fn unlock_file(&self, branch_id: &str, file_path: &str) -> Result<(), Box<dyn Error>>;
    fn check_file_locked(&self, branch_id: &str, file_path: &str) -> Result<String, Box<dyn Error>>;
}

pub struct SubmitManagerImpl {
    // 锁结构： Map<branch_id, Map<file_path, ticket>>
    lock_records: RwLock<HashMap<String, HashMap<String, Arc<String>>>>,
}

impl SubmitManagerImpl {
    pub fn new() -> Self {
        Self {
            lock_records: RwLock::new(HashMap::new()),
        }
    }
}

impl SubmitManager for SubmitManagerImpl {
    fn lock_file(&self, branch_id: &str, file_path: &str, ticket: String) -> Result<Arc<String>, Box<dyn Error>> {
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

pub(crate) fn submit_manager() -> &'static Arc<SubmitManagerImpl> {
    SUBMIT_MANAGER.get_or_init(|| Arc::new(SubmitManagerImpl::new()))
}

struct LockedFile {
    file_id: String,
    path: String,
    
}

struct  SubmitContext {
    ticket: String,
    files: Vec<LockedFile>,
}

