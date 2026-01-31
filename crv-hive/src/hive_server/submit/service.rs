use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use crate::common::depot_path::DepotPath;
use crate::hive_server::submit::cache_service;
use crate::hive_server::repository_manager;
use crate::caching::ChunkCacheError;
use crv_core::repository::{Compression, RepositoryError};

#[derive(Clone, Debug)]
pub struct LockedFile {
    /// depot path
    pub path: DepotPath,
    /// locked file generation, when file not exists, this should be None
    pub locked_generation: Option<i64>,
    /// locked file revision, when file not exists, this should be None
    pub locked_revision: Option<i64>,
}

#[derive(Clone, Debug)]
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
    /// chunks uploaded (completed)
    chunks_uploaded: RwLock<Vec<String>>,
    /// chunks in progress (including incomplete ones)
    chunks_in_progress: RwLock<HashSet<String>>,
}

pub struct SubmitService {
    /// locked files's paths
    locked_paths: RwLock<HashMap<DepotPath, uuid::Uuid>>,
    /// contexts of submitting
    contexts: RwLock<HashMap<uuid::Uuid, Arc<SubmitContext>>>,
}

#[derive(Debug)]
pub struct LaunchSubmitSuccess {
    pub ticket: uuid::Uuid,
}

#[derive(Debug)]
pub struct LaunchSubmitFailure {
    pub file_unable_to_lock: Vec<LockedFile>,
}

#[derive(Debug)]
pub struct FileRevision {
    pub path: String,
    pub generation: i64,
    pub revision: i64,
    pub binary_id: Vec<String>,
    pub size: i64,
    pub revision_created_at: i64,
}

#[derive(Debug)]
pub struct SubmitSuccess {
    pub changelist_id: i64,
    pub committed_at: i64,

    pub latest_revisions: Vec<FileRevision>,
    pub message: String,
}

#[derive(Debug)]
pub struct SubmitConflict {
    pub path: String,
    pub expected_generation: i64,
    pub expected_revision: i64,
    pub current_generation: i64,
    pub current_revision: i64,
}

#[derive(Debug)]
pub struct SubmitFailure {
    pub context_not_found: bool,
    pub conflicts: Vec<SubmitConflict>,
    pub missing_chunks: Vec<String>,
    pub message: String,
}

#[derive(Debug)]
pub enum UploadFileChunkResult {
    FileUploadFinished,
    FileAppended
}

#[derive(Debug)]
pub struct UploadFileChunkError {
    pub message: String
}

impl SubmitService {
    pub fn new() -> Self {
        Self {
            locked_paths: RwLock::new(HashMap::new()),
            contexts: RwLock::new(HashMap::new()),
        }
    }

    /// 仅用于单元测试：向 service 注入一个 context，避免依赖外部 DB / launch_submit。
    #[cfg(test)]
    pub(crate) fn insert_test_context(&self, ticket: uuid::Uuid) {
        let deadline = chrono::Utc::now() + chrono::Duration::minutes(10);
        let ctx = Arc::new(SubmitContext {
            ticket,
            submitting_by: "test".to_string(),
            timeout_deadline: deadline,
            files: Vec::new(),
            chunks_uploaded: RwLock::new(Vec::new()),
            chunks_in_progress: RwLock::new(HashSet::new()),
        });

        let mut contexts = self
            .contexts
            .write()
            .expect("submit service contexts poisoned");
        contexts.insert(ticket, ctx);
    }

    fn unlock_context(&self, ticket: &uuid::Uuid) {
        // 这里不依赖 contexts 里的 file 列表做定向删除，而是直接按 ticket 清除锁：
        // - 更稳健：即便 contexts 因异常路径缺失，也不会导致锁泄漏；
        // - 安全：只移除 value==ticket 的条目，不会误删其他并发 ticket 的锁。
        
        // 先获取需要清理的 chunk 列表（包括已上传和上传中的）
        let chunks_to_cleanup: HashSet<String> = {
            let contexts = self
                .contexts
                .read()
                .expect("submit service contexts poisoned");
            if let Some(ctx) = contexts.get(ticket) {
                let mut chunks = HashSet::new();
                
                // 添加已上传的 chunk
                let chunks_uploaded = ctx.chunks_uploaded.read()
                    .expect("submit service chunks_uploaded poisoned");
                chunks.extend(chunks_uploaded.iter().cloned());
                
                // 添加上传中的 chunk（可能包含未完成的）
                let chunks_in_progress = ctx.chunks_in_progress.read()
                    .expect("submit service chunks_in_progress poisoned");
                chunks.extend(chunks_in_progress.iter().cloned());
                
                chunks
            } else {
                HashSet::new()
            }
        };
        
        // 清理所有相关的 chunk cache（包括已上传和上传中的）
        let cache = cache_service();
        for chunk_hash in &chunks_to_cleanup {
            // 忽略删除错误，因为 chunk 可能已经被其他 ticket 使用或已被删除
            // 这些都是缓存文件，清理是安全的
            let _ = cache.remove_chunk(chunk_hash);
        }
        
        let mut locked = self
            .locked_paths
            .write()
            .expect("submit service locked_paths poisoned");
        locked.retain(|_, v| v != ticket);

        let mut context = self
            .contexts
            .write()
            .expect("submit service contexts poisoned");
        context.remove(ticket);
    }

    fn cleanup_expired_tickets(&self) {
        // 注意：这里绝不能在持有 `contexts` 写锁时调用 `unlock_context`，
        // 否则会在 `unlock_context` 内部二次申请 `contexts` 写锁导致自我死锁。
        //
        // 目前的解决方案：先在读锁下收集过期 ticket，释放锁后再逐个解锁。
        let now = chrono::Utc::now();
        let expired: Vec<uuid::Uuid> = {
            let contexts = self
                .contexts
                .read()
                .expect("submit service contexts poisoned");
            contexts
                .iter()
                .filter_map(|(ticket, ctx)| {
                    if ctx.timeout_deadline <= now {
                        Some(*ticket)
                    } else {
                        None
                    }
                })
                .collect()
        };

        for ticket in expired {
            self.unlock_context(&ticket);
        }
    }

    pub async fn launch_submit(
        &self,
        files: &Vec<LockedFile>,
        submitting_by: String,
        timeout: chrono::Duration,
    ) -> Result<LaunchSubmitSuccess, LaunchSubmitFailure> {
        // 进行周边工作，清理超时的 ticket
        self.cleanup_expired_tickets();

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
                chunks_uploaded: RwLock::new(Vec::new()),
                chunks_in_progress: RwLock::new(HashSet::new()),
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

    pub fn upload_file_chunk(&self, ticket: &uuid::Uuid, chunk_hash: &String, offset: i64, chunk_size: i64, bytes: &[u8]) -> Result<UploadFileChunkResult, UploadFileChunkError> {
        let contexts = self.contexts.read().expect("submit service contexts poisoned");
        let context = contexts.get(ticket);

        match context {
            Some(context_inner) => {
                let mut chunks_uploaded = context_inner.chunks_uploaded.write().expect("submit service chunks_uploaded poisoned");
                let mut chunks_in_progress = context_inner.chunks_in_progress.write().expect("submit service chunks_in_progress poisoned");
                
                // 将 chunk_hash 添加到正在进行的列表中（无论是否完成）
                chunks_in_progress.insert(chunk_hash.clone());
                
                // 使用 mod.rs 中的 CACHE_SERVICE 处理上传逻辑
                let cache = cache_service();
                
                // 将 offset 从 i64 转换为 u64
                let offset_u64 = offset.try_into().map_err(|_| UploadFileChunkError {
                    message: format!("invalid offset: {}", offset),
                })?;
                
                // 调用缓存服务写入 chunk 数据
                cache.append_chunk_part(chunk_hash, offset_u64, bytes).map_err(|e| {
                    UploadFileChunkError {
                        message: match e {
                            ChunkCacheError::InvalidChunkHash(msg) => format!("invalid chunk hash: {}", msg),
                            ChunkCacheError::Io(io_err) => format!("io error: {}", io_err),
                            ChunkCacheError::HashMismatch { expected, actual } => {
                                format!("hash mismatch: expected {}, actual {}", expected, actual)
                            }
                        },
                    }
                })?;
                
                // 判断当前写入是否已完成整个 chunk
                let bytes_written = bytes.len() as i64;
                let current_total_size = offset + bytes_written;
                
                // 检查是否超出预期大小
                if current_total_size > chunk_size {
                    return Err(UploadFileChunkError {
                        message: format!(
                            "chunk size exceeded: expected {}, actual {}",
                            chunk_size, current_total_size
                        ),
                    });
                }
                
                // 判断是否已完成整个 chunk
                let is_chunk_complete = current_total_size == chunk_size;
                
                // 如果 chunk 已完成，验证整个 chunk 的哈希值
                if is_chunk_complete {
                    
                    // 验证整个 chunk 的哈希值
                    match cache.has_chunk(chunk_hash) {
                        Ok(true) => {
                            // 哈希验证通过，chunk 上传完成
                            // 如果成功，将 chunk_hash 添加到已上传列表（去重）
                            if !chunks_uploaded.contains(chunk_hash) {
                                chunks_uploaded.push(chunk_hash.clone());
                            }
                            return Ok(UploadFileChunkResult::FileUploadFinished);
                        }
                        Ok(false) => {
                            // chunk 文件不存在（不应该发生，因为刚刚写入）
                            return Err(UploadFileChunkError {
                                message: format!("chunk file not found after write: {}", chunk_hash),
                            });
                        }
                        Err(e) => {
                            // 哈希验证失败
                            return Err(UploadFileChunkError {
                                message: match e {
                                    ChunkCacheError::InvalidChunkHash(msg) => {
                                        format!("invalid chunk hash during verification: {}", msg)
                                    }
                                    ChunkCacheError::Io(io_err) => {
                                        format!("io error during verification: {}", io_err)
                                    }
                                    ChunkCacheError::HashMismatch { expected, actual } => {
                                        format!(
                                            "chunk hash verification failed: expected {}, actual {}",
                                            expected, actual
                                        )
                                    }
                                },
                            });
                        }
                    }
                } else {
                    // chunk 尚未完成，只是追加了一部分数据
                    return Ok(UploadFileChunkResult::FileAppended);
                }
            }
            None => {
                return Result::Err(UploadFileChunkError{
                    message: "context not found".to_string(),
                });
            }
        }
    }

    /// ticket 是进行提交的上下文
    /// description 是提交的描述
    /// validations 是用于提交的验证，其中，key 是 depot path，value 是期望该文件在 cache 中已经完成上传的 chunk 的 hash 形成列表
    pub async fn submit(
        &self,
        ticket: &uuid::Uuid,
        description: String,
        validations: HashMap<DepotPath, Vec<String>>,
    ) -> Result<SubmitSuccess, SubmitFailure> {
        // 清理超时票据，避免长期占用锁
        self.cleanup_expired_tickets();

        let ctx: Arc<SubmitContext> = {
            let contexts = self
                .contexts
                .read()
                .expect("submit service contexts poisoned");
            let Some(ctx) = contexts.get(ticket) else {
                return Err(SubmitFailure {
                    context_not_found: true,
                    conflicts: vec![],
                    missing_chunks: vec![],
                    message: "context not found".to_string(),
                });
            };
            Arc::clone(ctx)
        };

        // 0) 检查 validations 覆盖了本次锁定的所有文件
        for f in &ctx.files {
            if !validations.contains_key(&f.path) {
                return Err(SubmitFailure {
                    context_not_found: false,
                    conflicts: vec![],
                    missing_chunks: vec![],
                    message: format!("missing validations for path: {}", f.path),
                });
            }
        }

        // 1) 再次检查版本冲突（即使 launch_submit 已检查过，也要防止跨实例/外部写入）
        let mut conflicts: Vec<SubmitConflict> = Vec::new();
        for f in &ctx.files {
            let expected_visible = match (f.locked_generation, f.locked_revision) {
                (Some(g), Some(r)) => Some((g, r)),
                (None, None) => None,
                _ => {
                    conflicts.push(SubmitConflict {
                        path: f.path.to_string(),
                        expected_generation: -1,
                        expected_revision: -1,
                        current_generation: -1,
                        current_revision: -1,
                    });
                    continue;
                }
            };

            let latest = match crate::database::dao::find_latest_file_revision_by_depot_path(
                &f.path.to_string(),
            )
            .await
            {
                Ok(m) => m,
                Err(e) => {
                    return Err(SubmitFailure {
                        context_not_found: false,
                        conflicts: vec![],
                        missing_chunks: vec![],
                        message: format!("database error while checking conflicts: {e}"),
                    });
                }
            };

            // “可见版本”：如果 latest 是 delete，则视为文件不存在（与 launch_submit 一致）
            let current_visible = latest.as_ref().and_then(|m| {
                if m.is_delete {
                    None
                } else {
                    Some((m.generation, m.revision))
                }
            });

            if expected_visible != current_visible {
                let (cur_g, cur_r) = latest
                    .as_ref()
                    .map(|m| (m.generation, m.revision))
                    .unwrap_or((0, 0));
                let (exp_g, exp_r) = expected_visible.unwrap_or((0, 0));
                conflicts.push(SubmitConflict {
                    path: f.path.to_string(),
                    expected_generation: exp_g,
                    expected_revision: exp_r,
                    current_generation: cur_g,
                    current_revision: cur_r,
                });
            }
        }

        if !conflicts.is_empty() {
            return Err(SubmitFailure {
                context_not_found: false,
                conflicts,
                missing_chunks: vec![],
                message: "submit conflict".to_string(),
            });
        }

        // 2) 检查 validations 描述的所有 chunk 都已完整存在于 cache（并通过 hash 校验）
        let cache = cache_service();
        let mut missing_chunks: Vec<String> = Vec::new();
        let mut unique_chunks: HashSet<String> = HashSet::new();

        for (_path, chunks) in validations.iter() {
            // 约定：空列表表示“删除该文件”，无需任何 chunk
            if chunks.is_empty() {
                continue;
            }
            for h in chunks {
                unique_chunks.insert(h.clone());
                match cache.has_chunk(h) {
                    Ok(true) => {}
                    Ok(false) => missing_chunks.push(h.clone()),
                    Err(_e) => {
                        // HashMismatch / IO 等都算“不可用”，直接按 missing 返回
                        missing_chunks.push(h.clone())
                    }
                }
            }
        }

        if !missing_chunks.is_empty() {
            missing_chunks.sort();
            missing_chunks.dedup();
            return Err(SubmitFailure {
                context_not_found: false,
                conflicts: vec![],
                missing_chunks,
                message: "missing chunks".to_string(),
            });
        }

        // 3) 将 chunk 写入 repository（写入成功或已存在都算通过）。
        //    同时记录每个 chunk 的长度，用于后续计算文件 size。
        let repo = match repository_manager() {
            Ok(r) => r,
            Err(e) => {
                return Err(SubmitFailure {
                    context_not_found: false,
                    conflicts: vec![],
                    missing_chunks: vec![],
                    message: format!("repository init error: {}", e.message()),
                });
            }
        };

        let mut chunk_sizes: HashMap<String, i64> = HashMap::new();
        for h in unique_chunks.iter() {
            let data = match cache.read_chunk(h) {
                Ok(b) => b,
                Err(e) => {
                    return Err(SubmitFailure {
                        context_not_found: false,
                        conflicts: vec![],
                        missing_chunks: vec![h.clone()],
                        message: format!("failed to read chunk from cache: {e}"),
                    });
                }
            };
            chunk_sizes.insert(h.clone(), data.len() as i64);

            match repo.write_chunk(&data, Compression::None) {
                Ok(_record) => {}
                Err(RepositoryError::DuplicateHash { .. }) => {
                    // repo 已存在该 chunk：视为 OK
                }
                Err(e) => {
                    return Err(SubmitFailure {
                        context_not_found: false,
                        conflicts: vec![],
                        missing_chunks: vec![],
                        message: format!("failed to write chunk into repository: {e}"),
                    });
                }
            }
        }

        // 4) 落库（changelist + file_revisions），成功后再删除 ticket/清理 cache
        let committed_at = chrono::Utc::now().timestamp();
        let author = ctx.submitting_by.clone();

        // 计算每个文件的新 generation/revision 与 size
        let mut revisions_to_insert: Vec<crate::database::dao::NewFileRevisionInput> = Vec::new();
        let mut latest_revisions: Vec<FileRevision> = Vec::new();

        for locked_file in &ctx.files {
            let depot_path = locked_file.path.to_string();
            let chunks = validations
                .get(&locked_file.path)
                .cloned()
                .unwrap_or_default();
            let is_delete = chunks.is_empty();

            let latest = crate::database::dao::find_latest_file_revision_by_depot_path(&depot_path)
                .await
                .map_err(|e| SubmitFailure {
                    context_not_found: false,
                    conflicts: vec![],
                    missing_chunks: vec![],
                    message: format!("database error while preparing revisions: {e}"),
                })?;

            let (new_generation, new_revision) = match latest {
                Some(m) => (m.generation, m.revision.saturating_add(1)),
                None => (1, 1),
            };

            let size: i64 = if is_delete {
                0
            } else {
                let mut size: i64 = 0;
                for h in &chunks {
                    if let Some(len) = chunk_sizes.get(h) {
                        size = size.saturating_add(*len);
                    }
                }
                size
            };

            let binary_id_json = serde_json::json!(chunks);
            revisions_to_insert.push(crate::database::dao::NewFileRevisionInput {
                depot_path: depot_path.clone(),
                generation: new_generation,
                revision: new_revision,
                binary_id: binary_id_json.clone(),
                size,
                is_delete,
                created_at: committed_at,
                metadata: serde_json::json!({}),
            });

            latest_revisions.push(FileRevision {
                path: depot_path,
                generation: new_generation,
                revision: new_revision,
                binary_id: binary_id_json
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
                size,
                revision_created_at: committed_at,
            });
        }

        let changes = serde_json::json!(
            revisions_to_insert
                .iter()
                .map(|r| serde_json::json!({
                    "path": r.depot_path,
                    "generation": r.generation,
                    "revision": r.revision,
                    "binary_id": r.binary_id,
                    "size": r.size,
                    "is_delete": r.is_delete,
                }))
                .collect::<Vec<_>>()
        );

        let changelist_id = match crate::database::dao::commit_submit(
            &author,
            &description,
            committed_at,
            changes,
            serde_json::json!({}),
            revisions_to_insert,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                // P0 修复：落库失败必须释放锁/上下文，否则会导致该 ticket 占用的文件锁长期不释放，
                // 后续提交会持续冲突（直到下一次触发 cleanup）。
                self.unlock_context(ticket);
                return Err(SubmitFailure {
                    context_not_found: false,
                    conflicts: vec![],
                    missing_chunks: vec![],
                    message: format!("database error while committing submit: {e}"),
                });
            }
        };

        // 5) 提交完成：删除 ticket 并清理 cache/释放锁
        self.unlock_context(ticket);

        Ok(SubmitSuccess {
            changelist_id,
            committed_at,
            latest_revisions,
            message: "success".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::OnceLock;

    use crate::database;
    use crate::database::entities;
    use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseBackend, Set, Statement};

    static INIT_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    fn should_run_hive_db_tests() -> bool {
        // 统一用环境变量控制：
        // - 默认不跑（本地/CI 都一样），避免没有 Postgres 环境时误失败
        // - `CRV_RUN_HIVE_DB_TESTS=1` => 允许运行（CI 可以显式开启，并配套启动 Postgres service）
        // - `CRV_SKIP_HIVE_DB_TESTS=1` => 强制跳过（用于临时禁用）
        if std::env::var("CRV_SKIP_HIVE_DB_TESTS").as_deref() == Ok("1") {
            eprintln!("skip submit service db tests (CRV_SKIP_HIVE_DB_TESTS=1)");
            return false;
        }

        if std::env::var("CRV_RUN_HIVE_DB_TESTS").as_deref() == Ok("1") {
            return true;
        }

        eprintln!(
            "skip submit service db tests (set CRV_RUN_HIVE_DB_TESTS=1 and run with --ignored)"
        );
        false
    }

    fn test_pg_config() -> crate::config::entity::ConfigEntity {
        // 允许通过环境变量覆盖，避免改源码才能在不同机器上跑。
        // 这些值只在 `CRV_RUN_HIVE_DB_TESTS=1` 时会被使用。
        let host = std::env::var("CRV_HIVE_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("CRV_HIVE_TEST_PG_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(5432);
        let db = std::env::var("CRV_HIVE_TEST_PG_DB").unwrap_or_else(|_| "chronoverse".into());
        let user = std::env::var("CRV_HIVE_TEST_PG_USER").unwrap_or_else(|_| "postgres".into());
        let pass = std::env::var("CRV_HIVE_TEST_PG_PASS").unwrap_or_else(|_| "postgres".into());

        let mut cfg = crate::config::entity::ConfigEntity::default();
        cfg.postgres_hostname = host;
        cfg.postgres_port = port;
        cfg.postgres_database = db;
        cfg.postgres_username = user;
        cfg.postgres_password = pass;
        cfg
    }

    async fn ensure_db() {
        if database::try_get().is_some() {
            return;
        }

        let m = INIT_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()));
        let _guard = m.lock().await;

        if database::try_get().is_some() {
            return;
        }

        let cfg = test_pg_config();
        let _ = crate::config::holder::try_set_config(cfg);

        // migrations 是幂等的。
        database::init().await.expect("db init");
    }

    fn unique_depot_file(name: &str) -> String {
        format!(
            "//tests/submit_service/{}/{}.txt",
            uuid::Uuid::new_v4(),
            name
        )
    }

    async fn insert_revision(
        depot_path: &str,
        generation: i64,
        revision: i64,
        is_delete: bool,
    ) -> i64 {
        ensure_db().await;
        let db = database::get();
        let backend = DatabaseBackend::Postgres;

        // 1) changelist（外键依赖）
        let cl = entities::changelists::ActiveModel {
            author: Set("test".to_string()),
            description: Set("test".to_string()),
            changes: Set(serde_json::json!([])),
            committed_at: Set(0),
            metadata: Set(serde_json::json!({})),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert changelist");

        // 2) file + 3) revision
        //
        // 注意：`files.path` / `file_revisions.path` 是 Postgres `ltree`，而 SeaORM 这里用 `String`
        // 绑定时会按 `text` 走，导致出现 “column path is of type ltree but expression is of type text”。
        // 测试辅助插入用 raw SQL 显式 `::ltree` cast，确保类型正确。
        let key = crate::database::ltree_key::depot_path_str_to_ltree_key(depot_path)
            .expect("encode depot path to ltree key");

        db.execute(Statement::from_sql_and_values(
            backend,
            r#"
            INSERT INTO files (path, created_at, metadata)
            VALUES ($1::ltree, 0, '{}'::jsonb)
            "#,
            [key.clone().into()].to_vec(),
        ))
        .await
        .expect("insert file");

        db.execute(Statement::from_sql_and_values(
            backend,
            r#"
            INSERT INTO file_revisions
                (path, generation, revision, changelist_id, binary_id, size, is_delete, created_at, metadata)
            VALUES
                ($1::ltree, $2, $3, $4, '{}'::jsonb, 0, $5, 0, '{}'::jsonb)
            "#,
            vec![
                key.into(),
                generation.into(),
                revision.into(),
                cl.id.into(),
                is_delete.into(),
            ],
        ))
        .await
        .expect("insert file revision");

        cl.id
    }

    async fn launch_submit_success_when_expected_none_and_no_db_record() {
        ensure_db().await;

        let depot_path = unique_depot_file("no_db_record");
        let p = DepotPath::new(&depot_path).unwrap();

        let svc = SubmitService::new();
        let files = vec![LockedFile {
            path: p,
            locked_generation: None,
            locked_revision: None,
        }];

        let r = svc
            .launch_submit(&files, "alice".to_string(), chrono::Duration::minutes(10))
            .await;

        assert!(r.is_ok(), "expected Ok, got: {:?}", r.err());
    }

    async fn launch_submit_rejects_duplicated_paths_in_request() {
        ensure_db().await;

        let depot_path = unique_depot_file("duplicated_paths");
        let p = DepotPath::new(&depot_path).unwrap();

        let svc = SubmitService::new();
        let files = vec![
            LockedFile {
                path: p.clone(),
                locked_generation: None,
                locked_revision: None,
            },
            LockedFile {
                path: p.clone(),
                locked_generation: None,
                locked_revision: None,
            },
        ];

        let r = svc
            .launch_submit(&files, "alice".to_string(), chrono::Duration::minutes(10))
            .await;

        assert!(r.is_err(), "expected Err");
        let e = r.err().unwrap();
        assert_eq!(e.file_unable_to_lock.len(), 1);
        assert_eq!(e.file_unable_to_lock[0].path.to_string(), depot_path);
    }

    async fn launch_submit_conflicts_when_already_locked_in_memory() {
        ensure_db().await;

        let depot_path = unique_depot_file("mem_lock_conflict");
        let p = DepotPath::new(&depot_path).unwrap();

        let svc = SubmitService::new();
        let files = vec![LockedFile {
            path: p.clone(),
            locked_generation: None,
            locked_revision: None,
        }];

        let first = svc
            .launch_submit(&files, "alice".to_string(), chrono::Duration::minutes(10))
            .await;
        assert!(first.is_ok(), "first should succeed");

        let second = svc
            .launch_submit(&files, "bob".to_string(), chrono::Duration::minutes(10))
            .await;
        assert!(second.is_err(), "second should conflict");
        let e = second.err().unwrap();
        assert_eq!(e.file_unable_to_lock.len(), 1);
        assert_eq!(e.file_unable_to_lock[0].path.to_string(), depot_path);
    }

    async fn launch_submit_rolls_back_locks_when_db_version_mismatch() {
        ensure_db().await;

        let depot_path = unique_depot_file("db_version_mismatch");
        insert_revision(&depot_path, 1, 2, false).await;

        // sanity: 确保 DB 里确实存在 (1,2) 且不是 delete
        let latest = crate::database::dao::find_latest_file_revision_by_depot_path(&depot_path)
            .await
            .expect("query latest revision")
            .expect("expected a latest revision");
        assert_eq!(latest.generation, 1);
        assert_eq!(latest.revision, 2);
        assert!(!latest.is_delete);

        let p = DepotPath::new(&depot_path).unwrap();
        let svc = SubmitService::new();

        // 期望版本不匹配：应失败，并且必须回滚释放锁（否则下一次会被内存锁挡住）
        let bad = vec![LockedFile {
            path: p.clone(),
            locked_generation: Some(1),
            locked_revision: Some(1),
        }];
        let r1 = svc
            .launch_submit(&bad, "alice".to_string(), chrono::Duration::minutes(10))
            .await;
        assert!(r1.is_err(), "expected mismatch to fail");
        assert!(
            svc.locked_paths
                .read()
                .expect("submit service locked_paths poisoned")
                .is_empty(),
            "expected locks to be rolled back after mismatch"
        );

        // 期望版本匹配：应成功（证明上一次失败后已释放 ticket 占用的锁）
        let good = vec![LockedFile {
            path: p.clone(),
            locked_generation: Some(1),
            locked_revision: Some(2),
        }];
        let r2 = svc
            .launch_submit(&good, "alice".to_string(), chrono::Duration::minutes(10))
            .await;
        assert!(
            r2.is_ok(),
            "expected rollback then succeed, got: {:?}",
            r2.err()
        );
    }

    async fn launch_submit_treats_deleted_latest_as_nonexistent() {
        ensure_db().await;

        let depot_path = unique_depot_file("deleted_latest");
        insert_revision(&depot_path, 9, 9, true).await;

        let p = DepotPath::new(&depot_path).unwrap();
        let svc = SubmitService::new();

        // latest 是 delete => current=None，因此 expected None 应成功
        let expected_none = vec![LockedFile {
            path: p.clone(),
            locked_generation: None,
            locked_revision: None,
        }];
        let r1 = svc
            .launch_submit(&expected_none, "alice".to_string(), chrono::Duration::minutes(10))
            .await;
        assert!(r1.is_ok());
    }

    /// 这些测试依赖全局单例数据库连接池（`crate::database::DB_CONN`），而 `#[tokio::test]`
    /// 默认会为每个测试创建并销毁一个独立 runtime，导致连接池跨 runtime 复用时出现
    /// “Tokio context ... is being shutdown”。
    ///
    /// 因此这里用一个共享 runtime 的单一 harness 串行执行。
    #[test]
    #[ignore = "requires external Postgres; enable with CRV_RUN_HIVE_DB_TESTS=1"]
    fn submit_service_tests_harness() {
        if !should_run_hive_db_tests() {
            return;
        }

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .expect("build tokio runtime for submit service tests");

        rt.block_on(async {
            launch_submit_success_when_expected_none_and_no_db_record().await;
            launch_submit_rejects_duplicated_paths_in_request().await;
            launch_submit_conflicts_when_already_locked_in_memory().await;
            launch_submit_rolls_back_locks_when_db_version_mismatch().await;
            launch_submit_treats_deleted_latest_as_nonexistent().await;
        });
    }
}