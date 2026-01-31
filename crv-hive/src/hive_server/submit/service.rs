use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use serde_json::Error;

use crate::common::depot_path::DepotPath;
use crate::hive_server::submit::cache_service;
use crate::caching::ChunkCacheError;

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

    pub fn submit(&self, ticket: &uuid::Uuid) -> Result<SubmitSuccess, SubmitFailure> {
        let context = self
            .contexts
            .write()
            .expect("submit service contexts poisoned");

        let context = context.get(ticket);
        if context.is_none() {
            return Result::Err(SubmitFailure {
                context_not_found: true,
                conflicts: vec![],
                missing_chunks: vec![],
                message: "context not found".to_string(),
            });
        }

        // todo: implement here

        return Result::Ok(SubmitSuccess {
            changelist_id: 0,
            committed_at: 0,
            latest_revisions: vec![],
            message: "success".to_string(),
        });
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

    // ============================
    // 直接在这里填写测试数据库地址
    // ============================
    const TEST_PG_HOST: &str = "172.18.168.1";
    const TEST_PG_PORT: u16 = 5432;
    const TEST_PG_DB: &str = "chronoverse";
    const TEST_PG_USER: &str = "postgres";
    const TEST_PG_PASS: &str = "postgres";

    async fn ensure_db() {
        if database::try_get().is_some() {
            return;
        }

        let m = INIT_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()));
        let _guard = m.lock().await;

        if database::try_get().is_some() {
            return;
        }

        // 通过“变量/常量”注入测试配置（不使用环境变量）
        let mut cfg = crate::config::entity::ConfigEntity::default();
        cfg.postgres_hostname = TEST_PG_HOST.to_string();
        cfg.postgres_port = TEST_PG_PORT;
        cfg.postgres_database = TEST_PG_DB.to_string();
        cfg.postgres_username = TEST_PG_USER.to_string();
        cfg.postgres_password = TEST_PG_PASS.to_string();
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
    fn submit_service_tests_harness() {
        // CI 默认不应运行依赖外部 Postgres 的测试（GitHub Actions 里没有这套环境）。
        // - GitHub Actions 会自动设置 `GITHUB_ACTIONS=true`
        // - 工作流里也额外设置了 `CRV_SKIP_HIVE_DB_TESTS=1`
        if std::env::var("GITHUB_ACTIONS").is_ok()
            || std::env::var("CRV_SKIP_HIVE_DB_TESTS").as_deref() == Ok("1")
        {
            eprintln!("skip submit service db tests on CI");
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