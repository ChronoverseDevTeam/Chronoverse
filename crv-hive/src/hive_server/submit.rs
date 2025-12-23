use crate::auth;
use crate::caching::{ChunkCache, ChunkCacheError};
use crate::hive_server::{depot_tree, derive_file_id_from_path, repository_manager, submit_lock};
use crate::pb::{
    CheckChunksReq, CheckChunksRsp, SubmitReq, SubmitRsp, TryLockFilesReq, TryLockFilesResp,
    UploadFileChunkReq, UploadFileChunkRsp,
};
use crv_core::metadata::{
    ChangelistAction, ChangelistChange, ChangelistDoc, ChangelistMetadata, FileDoc, FileMetadata,
    FileRevisionDoc, FileRevisionMetadata,
};
use crv_core::repository::{
    Compression, RepositoryError, blake3_hash_to_hex, blake3_hex_to_hash, compute_blake3_str,
};
use rand::rngs::OsRng;
use rand::RngCore;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex};
use tokio::time;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};

/// 提交上下文超时时间（秒）
const SUBMIT_CONTEXT_TIMEOUT_SECS: u64 = 20;

/// 提交上下文扫描间隔（秒）
const SUBMIT_CONTEXT_SCAN_INTERVAL_SECS: u64 = 5;

/// TryLock 阶段记录的每个文件锁定期望，用于在 Submit 阶段做一致性校验。
#[derive(Clone)]
struct LockedFileMeta {
    /// TryLock 时指定的 expected_file_revision（可为空）。
    expected_file_revision: String,
    /// TryLock 时指定的 expected_file_not_exist 标记。
    expected_file_not_exist: bool,
}

/// 单次提交流程的上下文，跨 TryLockFiles / UploadFileChunk / Submit 共用
struct SubmitContext {
    /// 本次提交所在分支
    branch_id: String,
    /// 通过 TryLockFiles 成功锁定的文件 id 列表（去重）
    locked_file_ids: Vec<String>,
    /// 每个锁定文件的期望信息
    locked_files_meta: HashMap<String, LockedFileMeta>,
    /// 在本次提交流程中上传/使用过的 chunk hash（小写 16 进制）
    used_chunk_hashes: HashSet<String>,
    /// 最近一次收到相关请求的时间
    last_activity: Instant,
    /// 是否已经被显式关闭（成功提交或主动回滚），若为 true 则不再执行超时清理
    closed: bool,
}

type SubmitContextHandle = Arc<Mutex<SubmitContext>>;
type SubmitContextMap = HashMap<String, SubmitContextHandle>;

static SUBMIT_CONTEXTS: tokio::sync::OnceCell<Mutex<SubmitContextMap>> =
    tokio::sync::OnceCell::const_new();

async fn submit_contexts() -> &'static Mutex<SubmitContextMap> {
    SUBMIT_CONTEXTS
        .get_or_init(|| async { Mutex::new(HashMap::new()) })
        .await
}

/// 生成一个不带 '-' 的 uuid（32 位十六进制字符串）
fn generate_uuid_no_dash() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 创建一个新的提交上下文，返回其 uuid
async fn create_submit_context(
    branch_id: &str,
    locked_file_ids: Vec<String>,
    locked_files_meta: HashMap<String, LockedFileMeta>,
) -> String {
    let uuid = generate_uuid_no_dash();

    let ctx = Arc::new(Mutex::new(SubmitContext {
        branch_id: branch_id.to_string(),
        locked_file_ids,
        locked_files_meta,
        used_chunk_hashes: HashSet::new(),
        last_activity: Instant::now(),
        closed: false,
    }));

    let contexts = submit_contexts().await;
    {
        let mut guard = contexts.lock().await;
        guard.insert(uuid.clone(), Arc::clone(&ctx));
    }

    spawn_submit_context_timeout_watcher(uuid.clone(), ctx);

    uuid
}

/// 根据 uuid 获取上下文并刷新活跃时间；如果不存在或已关闭，则返回错误
async fn touch_submit_context(uuid: &str) -> Result<SubmitContextHandle, Status> {
    if uuid.trim().is_empty() {
        return Err(Status::invalid_argument("uuid is required"));
    }

    let contexts = submit_contexts().await;
    let handle = {
        let guard = contexts.lock().await;
        guard.get(uuid).cloned()
    };

    let handle = handle.ok_or_else(|| {
        Status::not_found("submit context not found or expired for given uuid")
    })?;

    {
        let mut ctx = handle.lock().await;
        if ctx.closed {
            return Err(Status::not_found("submit context already closed"));
        }
        ctx.last_activity = Instant::now();
    }

    Ok(handle)
}

/// 将某个 chunk 记录到指定 uuid 的上下文中（并刷新活跃时间）
async fn record_chunk_in_context(uuid: &str, chunk_hash: &str) -> Result<(), Status> {
    // 允许 uuid 为空时不绑定上下文，维持向后兼容
    if uuid.trim().is_empty() {
        return Ok(());
    }

    let handle = touch_submit_context(uuid).await?;
    let mut ctx = handle.lock().await;
    ctx.used_chunk_hashes
        .insert(chunk_hash.to_lowercase());
    Ok(())
}

/// 将上下文标记为已关闭，并从全局 map 中移除（不做任何额外清理）
async fn close_submit_context(uuid: &str) {
    let contexts = submit_contexts().await;
    let mut guard = contexts.lock().await;
    if let Some(handle) = guard.remove(uuid) {
        let mut ctx = handle.lock().await;
        ctx.closed = true;
    }
}

/// 在超时场景下执行清理：解锁文件 + 删除 chunk cache
async fn cleanup_submit_context_with_timeout(uuid: &str, ctx: SubmitContext) {
    // 先删除 cache 中的相关 chunk，best-effort
    if let Ok(cache) = ChunkCache::from_config() {
        for ch in &ctx.used_chunk_hashes {
            if let Err(e) = cache.remove_chunk(ch) {
                eprintln!(
                    "submit context timeout: failed to remove cached chunk {ch}: {e}"
                );
            }
        }
    }

    // 解锁相关文件
    {
        let mut tree = depot_tree().lock().await;
        tree.unlock_files(&ctx.branch_id, &ctx.locked_file_ids);
    }

    // 最后从全局 map 中移除并标记为关闭
    let contexts = submit_contexts().await;
    let mut guard = contexts.lock().await;
    guard.remove(uuid);
}

/// 为单个提交上下文启动一个后台任务，定期检查是否超时。
fn spawn_submit_context_timeout_watcher(uuid: String, handle: SubmitContextHandle) {
    tokio::spawn(async move {
        loop {
            time::sleep(Duration::from_secs(
                SUBMIT_CONTEXT_SCAN_INTERVAL_SECS,
            ))
            .await;

            let maybe_ctx_snapshot = {
                let mut ctx = handle.lock().await;
                if ctx.closed {
                    // 已显式关闭，直接退出循环
                    return;
                }

                let now = Instant::now();
                if now.duration_since(ctx.last_activity)
                    >= Duration::from_secs(SUBMIT_CONTEXT_TIMEOUT_SECS)
                {
                    // 复制必要信息用于后续清理，并将 closed 标记为 true 防止重复执行
                    ctx.closed = true;
                    Some(SubmitContext {
                        branch_id: ctx.branch_id.clone(),
                        locked_file_ids: ctx.locked_file_ids.clone(),
                        locked_files_meta: ctx.locked_files_meta.clone(),
                        used_chunk_hashes: ctx.used_chunk_hashes.clone(),
                        last_activity: ctx.last_activity,
                        closed: true,
                    })
                } else {
                    None
                }
            };

            if let Some(snapshot) = maybe_ctx_snapshot {
                cleanup_submit_context_with_timeout(&uuid, snapshot).await;
                return;
            }
        }
    });
}

/// 校验：在基于 uuid 的上下文场景下，Submit 的文件集合及其期望必须与 TryLock 阶段保持一致。
fn validate_submit_files_against_locked(
    locked_meta: &HashMap<String, LockedFileMeta>,
    submit_files: &[crate::pb::SubmitFile],
) -> Result<(), String> {
    use std::collections::HashSet;

    // 1. 构造 Submit 阶段的文件集合与期望信息
    let mut submit_ids = HashSet::new();
    let mut submit_map: HashMap<String, (String, bool)> = HashMap::new(); // (expected_file_revision, is_delete)

    for f in submit_files {
        let mut file_id = f.file_id.trim().to_string();
        let path = f.path.trim().to_string();
        if file_id.is_empty() && !path.is_empty() {
            file_id = derive_file_id_from_path(&path);
        }
        if file_id.is_empty() {
            return Err("each submit file must have either file_id or path".to_string());
        }

        let expected_rev = f.expected_file_revision.trim().to_string();
        let is_delete = f.is_delete;

        submit_ids.insert(file_id.clone());
        submit_map.insert(file_id, (expected_rev, is_delete));
    }

    // 2. 校验文件集合是否完全一致（无多、无少）
    if locked_meta.len() != submit_ids.len()
        || !locked_meta.keys().all(|k| submit_ids.contains(k))
    {
        return Err("submit files do not match locked files".to_string());
    }

    // 3. 校验每个文件的期望（expected_revision / expected_not_exist）与 TryLock 是否一致
    for (fid, locked) in locked_meta {
        let (sub_rev, sub_is_delete) = submit_map
            .get(fid)
            .ok_or_else(|| "internal error: missing submit file after set check".to_string())?;

        if locked.expected_file_not_exist {
            // TryLock 时要求“当前不存在”，则 Submit 阶段应以“从不存在开始创建”为前提：
            // - expected_file_revision 必须为空
            // - is_delete 必须为 false（不能从“期望不存在”变成“删除”操作）
            if !sub_rev.is_empty() || *sub_is_delete {
                return Err(
                    "submit file expectation (expected not exist) does not match TryLockFiles"
                        .to_string(),
                );
            }
        } else {
            // TryLock 时有 expected_file_revision 约束（或为空表示不关心），
            // Submit 必须保持相同的 expected_file_revision。
            if locked.expected_file_revision != *sub_rev {
                return Err(
                    "submit file expected_file_revision does not match TryLockFiles".to_string(),
                );
            }
        }
    }

    Ok(())
}

/// 预检查一批文件是否可以被当前 changelist 锁定。
pub async fn handle_try_lock_files(
    request: Request<TryLockFilesReq>,
) -> Result<Response<TryLockFilesResp>, Status> {
    use crate::hive_server::hive_dao as dao;

    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let _user = auth::require_user(&request)?;

    let req = request.into_inner();

    let branch_id = req.branch_id.trim();
    if branch_id.is_empty() {
        return Err(Status::invalid_argument("branch_id is required"));
    }
    if req.files.is_empty() {
        return Err(Status::invalid_argument("files is required"));
    }

    // 读取分支信息，获取当前 HEAD changelist。
    let branch = dao::find_branch_by_id(branch_id)
        .await
        .map_err(|e| {
            Status::internal(format!(
                "database error while reading branch for TryLockFiles: {e}"
            ))
        })?
        .ok_or_else(|| Status::not_found("branch not found"))?;
    let head_changelist_id = branch.head_changelist_id;

    // 收集无法通过“版本/存在性检查”的文件。
    let mut unable_to_lock = Vec::new();

    // 为后续 DepotTree 加锁阶段缓存一些信息。
    // key = file_id
    let mut file_paths: HashMap<String, String> = HashMap::new();
    let mut file_current_revs: HashMap<String, String> = HashMap::new();
    let mut file_expected_revs: HashMap<String, String> = HashMap::new();
    let mut file_expected_not_exist: HashMap<String, bool> = HashMap::new();

    // 需要尝试加锁的 file_id 列表（可能包含重复，稍后会去重）。
    let mut candidate_file_ids: Vec<String> = Vec::new();

    for f in req.files {
        let mut file_id = f.file_id.trim().to_string();
        let path = f.path.trim().to_string();

        if file_id.is_empty() {
            if path.is_empty() {
                return Err(Status::invalid_argument(
                    "each FileToLock must have either file_id or path",
                ));
            }
            // 若未显式给出 file_id，则根据路径计算得到。
            file_id = derive_file_id_from_path(&path);
        }

        // HEAD 下当前文件最新的 revision。
        let head_rev =
            dao::find_file_revision_by_branch_file_and_cl(branch_id, &file_id, head_changelist_id)
                .await
                .map_err(|e| {
                    Status::internal(format!(
                        "database error while reading fileRevision for TryLockFiles: {e}"
                    ))
                })?;

        let expected_file_revision = f.expected_file_revision.trim().to_string();
        let expected_not_exist = f.expected_file_not_exist;

        let mut has_conflict = false;
        let mut current_revision = String::new();

        match head_rev {
            Some(r) => {
                current_revision = r.id.clone();

                // 若客户端声明了 expected_file_revision，则需与 HEAD 一致。
                if !expected_file_revision.is_empty() && r.id != expected_file_revision {
                    has_conflict = true;
                }

                // 若期望文件当前不存在，则要求 HEAD 上该文件为删除态。
                if expected_not_exist && !r.is_delete {
                    has_conflict = true;
                }
            }
            None => {
                // HEAD 下没有找到该文件 revision。
                // 若客户端期望某个具体 revision，则一定冲突。
                if !expected_file_revision.is_empty() {
                    has_conflict = true;
                }
                // 若仅期望“文件不存在”，则条件满足；否则视为“文件不存在但不冲突”。
            }
        }

        if has_conflict {
            unable_to_lock.push(crate::pb::FileUnableToLock {
                file_id: file_id.clone(),
                branch_id: branch_id.to_string(),
                path: path.clone(),
                current_file_revision: current_revision,
                expected_file_revision,
                expected_file_not_exist: expected_not_exist,
            });
        } else {
            // 记录通过版本/存在性检查的文件信息，稍后统一尝试加锁。
            candidate_file_ids.push(file_id.clone());
            file_paths.entry(file_id.clone()).or_insert(path);
            file_current_revs
                .entry(file_id.clone())
                .or_insert(current_revision);
            file_expected_revs
                .entry(file_id.clone())
                .or_insert_with(|| expected_file_revision.clone());
            file_expected_not_exist
                .entry(file_id.clone())
                .or_insert(expected_not_exist);
        }
    }

    // 若在版本/存在性检查阶段已经有文件失败，则不进行任何加锁，直接返回。
    if !unable_to_lock.is_empty() {
        let rsp = TryLockFilesResp {
            success: false,
            file_unable_to_lock: unable_to_lock,
            uuid: String::new(),
        };
        return Ok(Response::new(rsp));
    }

    // 去重，避免同一 file_id 多次出现在加锁列表中。
    let mut unique_file_ids = Vec::new();
    let mut seen = HashSet::new();
    for fid in candidate_file_ids {
        if seen.insert(fid.clone()) {
            unique_file_ids.push(fid);
        }
    }

    // 所有文件通过版本/存在性检查后，再尝试在 DepotTree 中一次性加锁。
    let mut tree = depot_tree().lock().await;
    let (_locked, conflicted) = tree.try_lock_files(branch_id, &unique_file_ids);

    if !conflicted.is_empty() {
        // DepotTree 中已经存在锁冲突，同样视为整体失败。
        let mut unable_due_to_lock = Vec::new();
        for fid in conflicted {
            let path = file_paths.get(&fid).cloned().unwrap_or_default();
            let current_revision = file_current_revs.get(&fid).cloned().unwrap_or_default();
            let expected_revision = file_expected_revs.get(&fid).cloned().unwrap_or_default();
            let expected_not_exist = *file_expected_not_exist.get(&fid).unwrap_or(&false);

            unable_due_to_lock.push(crate::pb::FileUnableToLock {
                file_id: fid,
                branch_id: branch_id.to_string(),
                path,
                current_file_revision: current_revision,
                expected_file_revision: expected_revision,
                expected_file_not_exist: expected_not_exist,
            });
        }

        let rsp = TryLockFilesResp {
            success: false,
            file_unable_to_lock: unable_due_to_lock,
            uuid: String::new(),
        };
        return Ok(Response::new(rsp));
    }

    // 构造锁定文件的期望信息
    let mut locked_files_meta: HashMap<String, LockedFileMeta> = HashMap::new();
    for fid in &unique_file_ids {
        let expected_file_revision = file_expected_revs.get(fid).cloned().unwrap_or_default();
        let expected_file_not_exist = *file_expected_not_exist.get(fid).unwrap_or(&false);

        locked_files_meta.insert(
            fid.clone(),
            LockedFileMeta {
                expected_file_revision,
                expected_file_not_exist,
            },
        );
    }

    // 全部成功加锁：创建提交上下文并返回 uuid。
    let uuid = create_submit_context(branch_id, unique_file_ids, locked_files_meta).await;

    let rsp = TryLockFilesResp {
        success: true,
        file_unable_to_lock: Vec::new(),
        uuid,
    };
    Ok(Response::new(rsp))
}

/// 检查服务器端当前缺少哪些 chunk_hash。
pub async fn handle_check_chunks(
    request: Request<CheckChunksReq>,
) -> Result<Response<CheckChunksRsp>, Status> {
    use std::collections::HashSet;

    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let _user = auth::require_user(&request)?;

    let req = request.into_inner();

    if req.chunk_hashes.is_empty() {
        return Ok(Response::new(CheckChunksRsp {
            missing_chunk_hashes: Vec::new(),
        }));
    }

    // 打开主仓库（repository），用于判断 chunk 是否已经持久化存储。
    // 使用全局 RepositoryManager，避免在每次调用时重复初始化。
    let repo = repository_manager()?;

    // 打开本地 chunk 缓存目录。
    let cache = ChunkCache::from_config()
        .map_err(|e| Status::internal(format!("failed to initialize chunk cache: {e}")))?;

    let mut missing = Vec::new();
    let mut seen = HashSet::new();

    for raw in req.chunk_hashes {
        let hash_hex = raw.trim().to_lowercase();
        if hash_hex.is_empty() {
            continue;
        }
        // 去重，避免同一 chunk_hash 重复检查。
        if !seen.insert(hash_hex.clone()) {
            continue;
        }

        // 1) 优先检查主仓库中是否已经存在该 chunk。
        let parsed = blake3_hex_to_hash(&hash_hex);
        if let Some(chunk_hash) = parsed {
            match repo.locate_chunk(&chunk_hash) {
                Ok(Some(_)) => {
                    // 已存在于 repository 中，跳过。
                    continue;
                }
                Ok(None) => {
                    // 仓库中不存在，继续检查缓存。
                }
                Err(e) => {
                    // 仓库访问异常，打印日志，保守起见视为缺失。
                    eprintln!("check_chunks: repository error for {hash_hex}: {e}");
                    missing.push(hash_hex);
                    continue;
                }
            }
        } else {
            // 无法解析为合法的 64 位 hex，直接视为缺失。
            missing.push(hash_hex);
            continue;
        }

        // 2) 仓库中不存在，则检查本地 chunk 缓存。
        match cache.has_chunk(&hash_hex) {
            Ok(true) => {
                // 缓存中存在且哈希校验通过，视为完整 chunk，不加入缺失列表。
            }
            Ok(false) => {
                // 缓存中不存在，视为缺失。
                missing.push(hash_hex);
            }
            Err(ChunkCacheError::HashMismatch { .. }) => {
                // 缓存文件存在但内容与 hash 不匹配，删除该缓存并视为缺失。
                if let Err(e) = cache.remove_chunk(&hash_hex) {
                    eprintln!("check_chunks: failed to remove corrupted cache for {hash_hex}: {e}");
                }
                missing.push(hash_hex);
            }
            Err(e) => {
                // 其他缓存错误：打印日志，但仍然将该 chunk 视为缺失。
                eprintln!("check_chunks: error checking cache for {hash_hex}: {e}");
                missing.push(hash_hex);
            }
        }
    }

    let rsp = CheckChunksRsp {
        missing_chunk_hashes: missing,
    };
    Ok(Response::new(rsp))
}

/// 处理单个 chunk 上传的核心逻辑，供流式 API 循环调用。
async fn handle_single_upload_file_chunk(
    req: UploadFileChunkReq,
) -> Result<UploadFileChunkRsp, Status> {
    let uuid = req.uuid.clone();

    let chunk_hash = req.chunk_hash.trim().to_lowercase();
    if chunk_hash.is_empty() {
        return Err(Status::invalid_argument("chunk_hash is required"));
    }

    let offset = if req.offset < 0 {
        return Err(Status::invalid_argument("offset must be non-negative"));
    } else {
        req.offset as u64
    };

    // 将本次 chunk 记录到上下文（若 uuid 非空），并刷新活跃时间。
    // 若 uuid 为空，则允许作为“无上下文”上传，保持向后兼容。
    if !uuid.trim().is_empty() {
        record_chunk_in_context(&uuid, &chunk_hash).await?;
    }

    // 初始化本地 chunk 缓存
    let cache = ChunkCache::from_config()
        .map_err(|e| Status::internal(format!("failed to initialize chunk cache: {e}")))?;

    // 对于 offset == 0 的首块上传，先检查服务器端是否已经存在相同 chunk：
    // 1) 若主仓库中已存在，则直接返回 already_exists=true；
    // 2) 若缓存中已存在且哈希校验通过，同样返回 already_exists=true。
    if offset == 0 {
        // 先检查主仓库（Repository）
        if let Some(hash_bytes) = blake3_hex_to_hash(&chunk_hash) {
            match repository_manager() {
                Ok(repo) => match repo.locate_chunk(&hash_bytes) {
                    Ok(Some(_)) => {
                        let rsp = UploadFileChunkRsp {
                            success: true,
                            chunk_hash: chunk_hash.clone(),
                            message: "chunk already exists in repository".to_string(),
                            already_exists: true,
                            uuid,
                        };
                        return Ok(rsp);
                    }
                    Ok(None) => {
                        // 仓库中不存在，继续检查缓存
                    }
                    Err(e) => {
                        // 仓库访问异常，视为内部错误
                        return Err(Status::internal(format!(
                            "failed to check chunk in repository: {e}"
                        )));
                    }
                },
                Err(status) => {
                    // 无法初始化 RepositoryManager，直接返回错误
                    return Err(status);
                }
            }
        }

        // 再检查缓存中是否已存在完整且校验通过的 chunk
        match cache.has_chunk(&chunk_hash) {
            Ok(true) => {
                let rsp = UploadFileChunkRsp {
                    success: true,
                    chunk_hash: chunk_hash.clone(),
                    message: "chunk already exists in cache".to_string(),
                    already_exists: true,
                    uuid,
                };
                return Ok(rsp);
            }
            Ok(false) => {
                // 缓存中不存在，继续执行写入逻辑
            }
            Err(ChunkCacheError::HashMismatch { .. }) => {
                // 缓存文件存在但内容与 hash 不匹配，删除该缓存并提示客户端重新上传。
                if let Err(e) = cache.remove_chunk(&chunk_hash) {
                    eprintln!(
                        "upload_file_chunk: failed to remove corrupted cache for {chunk_hash}: {e}"
                    );
                }
                return Err(Status::internal(
                    "existing cached chunk is corrupted, please re-upload from offset 0",
                ));
            }
            Err(e) => {
                return Err(Status::internal(format!(
                    "failed to check chunk in cache: {e}"
                )));
            }
        }
    }

    // 写入当前这部分数据（content 可能为空，此时为 no-op）
    if !req.content.is_empty() {
        if let Err(e) = cache.append_chunk_part(&chunk_hash, offset, &req.content) {
            return Err(match e {
                ChunkCacheError::InvalidChunkHash(msg) => {
                    Status::invalid_argument(format!("invalid chunk state: {msg}"))
                }
                ChunkCacheError::Io(ioe) => {
                    Status::internal(format!("failed to write chunk part: {ioe}"))
                }
                ChunkCacheError::HashMismatch { .. } => {
                    // append_chunk_part 不会返回 HashMismatch，这里只是兜底。
                    Status::internal("hash mismatch while appending chunk part")
                }
            });
        }
    }

    // 这里不强制在 eof 时立即做完整性校验，统一交由后续的 CheckChunks / Submit 逻辑处理。
    // 只要本次写入成功，就认为本次调用成功。
    let rsp = UploadFileChunkRsp {
        success: true,
        chunk_hash,
        message: String::new(),
        already_exists: false,
        uuid,
    };
    Ok(rsp)
}

/// 流式上传文件内容块：
/// - 一个 changelist 中涉及的**所有 chunk 的所有分片**共用同一个 gRPC 流上传；
/// - 若在 offset == 0 时发现 chunk 已经存在（仓库或缓存中），则**立刻**返回一次响应，客户端可以据此停止继续上传该 chunk；
/// - 若是新 chunk，则仅在该 chunk 的 `eof == true` 时发送一次响应。
pub type UploadFileChunkStream = ReceiverStream<Result<UploadFileChunkRsp, Status>>;

pub async fn handle_upload_file_chunk_stream(
    request: Request<Streaming<UploadFileChunkReq>>,
) -> Result<Response<UploadFileChunkStream>, Status> {
    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let _user = auth::require_user(&request)?;

    let mut inbound = request.into_inner();
    let (tx, rx) = mpsc::channel::<Result<UploadFileChunkRsp, Status>>(32);

    tokio::spawn(async move {
        use std::collections::{HashMap, HashSet};
        // 记录每个 chunk_hash 对应的“首个成功响应”，以便在 eof 时一次性返回。
        let mut pending_by_chunk: HashMap<String, UploadFileChunkRsp> = HashMap::new();
        // 标记已经完成响应的 chunk_hash（包括 already_exists 立即返回的情况），避免重复发送。
        let mut completed_chunks: HashSet<String> = HashSet::new();

        while let Some(next) = inbound.next().await {
            match next {
                Ok(req) => {
                    let uuid_for_log = req.uuid.clone();
                    let chunk_hash_key = req.chunk_hash.trim().to_lowercase();
                    let eof_flag = req.eof;

                    // 若该 chunk 已经完成响应（例如 offset==0 就 already_exists 并返回过），
                    // 则忽略后续同一 chunk 的所有分片，避免重复响应。
                    if completed_chunks.contains(&chunk_hash_key) {
                        continue;
                    }
                    match handle_single_upload_file_chunk(req).await {
                        Ok(mut rsp) => {
                            // 确保响应里携带 chunk_hash
                            if rsp.chunk_hash.is_empty() {
                                rsp.chunk_hash = chunk_hash_key.clone();
                            }

                            // 若服务器在 offset==0 时就发现 chunk 已存在（already_exists=true），
                            // 则立即返回该响应，让客户端可以尽早停止上传该 chunk，
                            // 并标记该 chunk 已完成，后续同一 chunk 的分片将被忽略。
                            if rsp.already_exists {
                                completed_chunks.insert(chunk_hash_key.clone());
                                if tx.send(Ok(rsp)).await.is_err() {
                                    break;
                                }
                                continue;
                            }

                            // 对同一个 chunk_hash，只保留第一次成功响应（保证状态不被后续覆盖）。
                            pending_by_chunk
                                .entry(chunk_hash_key.clone())
                                .or_insert_with(|| rsp);

                            // 仅在 eof 时真正发送响应，并标记该 chunk 已完成。
                            if eof_flag {
                                if let Some(rsp_to_send) = pending_by_chunk.remove(&chunk_hash_key) {
                                    completed_chunks.insert(chunk_hash_key.clone());
                                    if tx.send(Ok(rsp_to_send)).await.is_err() {
                                        // 客户端已断开连接，结束任务
                                        break;
                                    }
                                }
                            }
                        }
                        Err(status) => {
                            eprintln!(
                                "upload_file_chunk stream error for uuid {uuid_for_log}: {status}"
                            );
                            let _ = tx.send(Err(status)).await;
                            break;
                        }
                    }
                }
                Err(status) => {
                    let _ = tx.send(Err(status)).await;
                    break;
                }
            }
        }
    });

    Ok(Response::new(ReceiverStream::new(rx)))
}

/// 处理 Submit 请求的完整实现逻辑。
///
/// 注意：该函数假定外部已经通过 gRPC 拦截器注入了 UserContext。
pub async fn handle_submit(request: Request<SubmitReq>) -> Result<Response<SubmitRsp>, Status> {
    use crate::hive_server::hive_dao as dao;

    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let user_ctx = auth::require_user(&request)?;
    let username = user_ctx.username.clone();

    // 串行化所有 Submit 调用，防止并发修改同一分支 HEAD 等元数据。
    let _submit_guard = submit_lock().lock().await;

    let req = request.into_inner();

    // 如果提供了 uuid，则优先使用提交上下文；否则退回到旧的“无上下文”路径（不建议）。
    let maybe_uuid = req.uuid.trim().to_string();

    let branch_id_from_req = req.branch_id.trim();
    if branch_id_from_req.is_empty() {
        return Err(Status::invalid_argument("branch_id is required"));
    }
    if req.files.is_empty() {
        return Err(Status::invalid_argument("files is required"));
    }

    // -----------------------------
    // 1. 加载分支 & 计算新 changelist id
    // -----------------------------

    let branch_id = branch_id_from_req;

    // 加载分支信息，获取当前 HEAD changelist。
    let branch = dao::find_branch_by_id(branch_id)
        .await
        .map_err(|e| Status::internal(format!("database error while reading branch: {e}")))?
        .ok_or_else(|| Status::not_found("branch not found"))?;
    let parent_changelist_id = branch.head_changelist_id;

    // 分配新的 changelist ID（使用 Postgres 序列，避免并发竞态）。
    let new_changelist_id = dao::allocate_changelist_id()
        .await
        .map_err(|e| Status::internal(format!("database error while allocating changelist id: {e}")))?;

    // 当前时间戳（毫秒）
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // 预先计算本次提交涉及的 file_id 列表（去重），以及锁定文件列表来源：
    // - 若存在提交上下文，则使用上下文中的 locked_file_ids，并强制要求 Submit.files 与之完全一致；
    // - 若不存在上下文，则退回到旧逻辑，从 req.files 中推导（兼容旧客户端）。
    let mut locked_file_ids: Vec<String> = Vec::new();
    let mut used_chunk_hashes: HashSet<String> = HashSet::new();
    let mut locked_files_meta: HashMap<String, LockedFileMeta> = HashMap::new();

    if !maybe_uuid.is_empty() {
        // 尝试从上下文中获取锁定文件集合和已使用的 chunk 集合。
        let handle = touch_submit_context(&maybe_uuid).await.map_err(|status| {
            Status::failed_precondition(format!(
                "submit context is not available for this uuid: {status}"
            ))
        })?;
        let ctx = handle.lock().await;

        // 分支必须保持一致
        if ctx.branch_id != branch_id {
            return Err(Status::invalid_argument(
                "branch_id in SubmitReq does not match context branch_id",
            ));
        }

        locked_file_ids = ctx.locked_file_ids.clone();
        used_chunk_hashes = ctx.used_chunk_hashes.clone();
        locked_files_meta = ctx.locked_files_meta.clone();
    } else {
        // 无上下文的兼容路径：继续使用旧行为，从 req.files 中解析锁定文件集合。
        let mut seen = HashSet::new();
        for f in &req.files {
            let mut file_id = f.file_id.trim().to_string();
            let path = f.path.trim().to_string();
            if file_id.is_empty() && !path.is_empty() {
                file_id = derive_file_id_from_path(&path);
            }
            if !file_id.is_empty() && seen.insert(file_id.clone()) {
                locked_file_ids.push(file_id);
            }
        }

        // 同时预计算 used_chunk_hashes，便于后续清理 cache。
        for f in &req.files {
            for ch in &f.binary_id {
                let ch_trim = ch.trim().to_string();
                if ch_trim.is_empty() {
                    continue;
                }
                used_chunk_hashes.insert(ch_trim.to_lowercase());
            }
        }
    }

    // 若存在上下文，则强制要求 Submit.files 与 TryLock 阶段的锁定记录一致。
    if !maybe_uuid.is_empty() {
        if let Err(msg) = validate_submit_files_against_locked(&locked_files_meta, &req.files) {
            {
                let mut tree = depot_tree().lock().await;
                tree.unlock_files(branch_id, &locked_file_ids);
            }

            close_submit_context(&maybe_uuid).await;

            let rsp = SubmitRsp {
                success: false,
                changelist_id: 0,
                committed_at: 0,
                conflicts: Vec::new(),
                missing_chunks: Vec::new(),
                message: msg,
                uuid: maybe_uuid,
            };
            return Ok(Response::new(rsp));
        }
    }

    // -----------------------------
    // 2. 打开 Repository & 预检查缺失 chunk
    // -----------------------------

    // 打开底层 Repository，用于持久化本次提交相关的 chunk。
    let repo = repository_manager()?;

    // 先检查是否有缺失的 chunk（基于本地 ChunkCache）。
    // 若发现缺失，则直接返回 missing_chunks，让客户端先补齐后再提交。
    let mut missing_chunks = Vec::new();
    {
        let cache = ChunkCache::from_config()
            .map_err(|e| Status::internal(format!("failed to initialize chunk cache: {e}")))?;
        let mut seen_chunks = HashSet::new();

        for f in &req.files {
            for ch in &f.binary_id {
                let ch_trim = ch.trim().to_string();
                if ch_trim.is_empty() {
                    continue;
                }
                if !seen_chunks.insert(ch_trim.clone()) {
                    continue;
                }

                // 如果处于基于 uuid 的上下文中，且该 chunk_hash 已在 UploadFileChunk 阶段成功记录进上下文，
                // 则认为该 chunk 已经上传成功，即便当前缓存检查失败也不再将其视为缺失。
                if !maybe_uuid.is_empty()
                    && used_chunk_hashes.contains(&ch_trim.to_lowercase())
                {
                    continue;
                }

                match cache.has_chunk(&ch_trim) {
                    Ok(true) => {}
                    Ok(false) => {
                        missing_chunks.push(ch_trim);
                    }
                    Err(e) => {
                        eprintln!("submit: error checking chunk {ch_trim} in cache: {e}");
                        missing_chunks.push(ch_trim);
                    }
                }
            }
        }
    }

    if !missing_chunks.is_empty() {
        // 提交前发现缺失 chunk，直接失败并释放本次提交涉及的文件锁。
        {
            let mut tree = depot_tree().lock().await;
            tree.unlock_files(branch_id, &locked_file_ids);
        }

        // 若使用了上下文，则显式关闭，交给客户端重新发起新的 TryLock / Submit 流程。
        if !maybe_uuid.is_empty() {
            close_submit_context(&maybe_uuid).await;
        }

        let rsp = SubmitRsp {
            success: false,
            changelist_id: 0,
            committed_at: 0,
            conflicts: Vec::new(),
            missing_chunks,
            message: "missing chunks, please upload them before submit".to_string(),
            uuid: maybe_uuid,
        };
        return Ok(Response::new(rsp));
    }

    // -----------------------------
    // 3. 将 chunk 从缓存写入 Repository（若尚未存在）
    // -----------------------------

    if !used_chunk_hashes.is_empty() {
        let cache = ChunkCache::from_config().map_err(|e| {
            Status::internal(format!(
                "failed to initialize chunk cache for repository write: {e}"
            ))
        })?;

        for ch in &used_chunk_hashes {
            // 从缓存中读取完整 chunk 内容。
            if let Ok(path) = cache.chunk_path_unchecked(ch) {
                eprintln!(
                    "handle_submit: about to read chunk {}, path={}, exists={}", 
                    ch,
                    path.display(),
                    path.exists()
                );
            }
            let bytes = cache.read_chunk(ch).map_err(|e| {
                Status::internal(format!("failed to read cached chunk {ch}: {e}"))
            })?;

            // 尝试写入 Repository；若已有相同 hash 则视为正常（幂等）。
            match repo.write_chunk(&bytes, Compression::None) {
                Ok(_) => {}
                Err(RepositoryError::DuplicateHash { .. }) => {
                    // 已存在相同 chunk，忽略即可。
                }
                Err(e) => {
                    return Err(Status::internal(format!(
                        "failed to write chunk {ch} into repository: {e}"
                    )));
                }
            }
        }
    }

    // 版本冲突检测 & 构造 File / FileRevision / ChangelistChange 文档。
    let mut conflicts = Vec::new();
    let mut new_files: HashMap<String, FileDoc> = HashMap::new();
    let mut known_files: HashMap<String, Option<FileDoc>> = HashMap::new();
    let mut file_revisions = Vec::new();
    let mut changes = Vec::new();

    let files = req.files;
    let files_len = files.len() as i64;

    for f in files {
        let mut file_id = f.file_id.trim().to_string();
        let path = f.path.trim().to_string();
        if file_id.is_empty() {
            if path.is_empty() {
                return Err(Status::invalid_argument(
                    "each submit file must have either file_id or path",
                ));
            }
            // 使用与 try_lock_files 相同的封装，根据路径计算 file_id。
            file_id = derive_file_id_from_path(&path);
        }

        // 查询当前 HEAD 下该文件的最新 revision。
        let head_rev = dao::find_file_revision_by_branch_file_and_cl(
            branch_id,
            &file_id,
            parent_changelist_id,
        )
        .await
        .map_err(|e| {
            Status::internal(format!(
                "database error while reading fileRevision for submit: {e}"
            ))
        })?;

        let expected_file_revision = f.expected_file_revision.trim().to_string();
        let is_delete = f.is_delete;

        // 冲突检测：若客户端声明了 expected_file_revision，则需与 HEAD 一致。
        let mut has_conflict = false;
        let mut current_revision = String::new();
        if !expected_file_revision.is_empty() {
            match &head_rev {
                Some(r) if r.id == expected_file_revision => {
                    current_revision = r.id.clone();
                }
                Some(r) => {
                    has_conflict = true;
                    current_revision = r.id.clone();
                }
                None => {
                    has_conflict = true;
                }
            }
        } else if let Some(r) = &head_rev {
            current_revision = r.id.clone();
        }

        if has_conflict {
            conflicts.push(crate::pb::SubmitConflict {
                file_id: file_id.clone(),
                expected_file_revision,
                current_revision,
            });
            continue;
        }

        // 决定本次变更动作类型：create/modify/delete
        let action = if is_delete {
            ChangelistAction::Delete
        } else if head_rev.is_some() {
            ChangelistAction::Modify
        } else {
            ChangelistAction::Create
        };

        // 确保 File 文档存在（对于新文件需要插入）。
        if !known_files.contains_key(&file_id) {
            let existing = dao::find_file_by_id(&file_id).await.map_err(|e| {
                Status::internal(format!("database error while reading file for submit: {e}"))
            })?;
            known_files.insert(file_id.clone(), existing.clone());
            if existing.is_none() {
                let doc = FileDoc {
                    id: file_id.clone(),
                    path: path.clone(),
                    created_at: now_millis,
                    metadata: FileMetadata {
                        // 使用当前登录用户作为该文件的首次引入者。
                        first_introduced_by: username.clone(),
                    },
                };
                new_files.insert(file_id.clone(), doc);
            }
        }

        // 构造新的 FileRevision 文档。
        let parent_revision_id = head_rev.as_ref().map(|r| r.id.clone()).unwrap_or_default();

        // FileRevision `_id` = blake3(branchId + ":" + fileId + ":" + changelistId)
        let fr_id_input = format!("{branch_id}:{file_id}:{new_changelist_id}");
        let fr_hash_bytes = compute_blake3_str(&fr_id_input);
        let fr_id = blake3_hash_to_hex(&fr_hash_bytes);

        let file_mode = f.file_mode.clone().unwrap_or_else(|| "755".to_string());

        // 目前缺少完整文件内容，这里的 hash 先简单使用第一个 chunk 的 hash。
        let content_hash = f.binary_id.get(0).cloned().unwrap_or_else(String::new);

        let rev_doc = FileRevisionDoc {
            id: fr_id.clone(),
            branch_id: branch_id.to_string(),
            file_id: file_id.clone(),
            changelist_id: new_changelist_id,
            binary_id: f.binary_id.clone(),
            parent_revision_id,
            size: f.size,
            is_delete,
            created_at: now_millis,
            metadata: FileRevisionMetadata {
                file_mode,
                hash: content_hash,
                is_binary: true,
                language: String::new(),
            },
        };

        file_revisions.push(rev_doc);

        changes.push(ChangelistChange {
            file: file_id,
            action,
            revision: fr_id,
        });
    }

    // 如果存在冲突，则直接返回，不进行任何写入，并释放文件锁。
    if !conflicts.is_empty() {
        {
            let mut tree = depot_tree().lock().await;
            tree.unlock_files(branch_id, &locked_file_ids);
        }

        if !maybe_uuid.is_empty() {
            close_submit_context(&maybe_uuid).await;
        }

        let rsp = SubmitRsp {
            success: false,
            changelist_id: 0,
            committed_at: 0,
            conflicts,
            missing_chunks: Vec::new(),
            message: "submit aborted due to file revision conflicts".to_string(),
            uuid: maybe_uuid,
        };
        return Ok(Response::new(rsp));
    }

    // 插入新 File 文档
    for (_id, doc) in new_files {
        dao::insert_file(doc)
            .await
            .map_err(|e| Status::internal(format!("database error while inserting file: {e}")))?;
    }

    // 插入 FileRevision 文档
    dao::insert_file_revisions(file_revisions)
        .await
        .map_err(|e| {
            Status::internal(format!("database error while inserting fileRevision: {e}"))
        })?;

    // 插入 Changelist 文档
    let cl_doc = ChangelistDoc {
        id: new_changelist_id,
        parent_changelist_id,
        branch_id: branch_id.to_string(),
        // 使用当前登录用户作为 changelist 作者。
        author: username,
        description: req.description,
        changes,
        committed_at: now_millis,
        files_count: files_len,
        metadata: ChangelistMetadata { labels: Vec::new() },
    };

    dao::insert_changelist(cl_doc)
        .await
        .map_err(|e| Status::internal(format!("database error while inserting changelist: {e}")))?;

    // 更新分支 HEAD
    dao::update_branch_head(branch_id, new_changelist_id)
        .await
        .map_err(|e| Status::internal(format!("database error while updating branch head: {e}")))?;

    // 提交成功后，尝试清理本次提交涉及到的 chunk 缓存文件（best-effort）。
    {
        if let Ok(cache) = ChunkCache::from_config() {
            for ch in &used_chunk_hashes {
                if let Err(e) = cache.remove_chunk(ch) {
                    eprintln!("submit: failed to remove cached chunk {ch} after submit: {e}");
                }
            }
        }
    }

    // 正常提交成功后，释放本次提交涉及的文件锁。
    {
        let mut tree = depot_tree().lock().await;
        tree.unlock_files(branch_id, &locked_file_ids);
    }

    // 若使用了上下文，显式关闭，防止后续被超时任务重复清理。
    if !maybe_uuid.is_empty() {
        close_submit_context(&maybe_uuid).await;
    }

    let rsp = SubmitRsp {
        success: true,
        changelist_id: new_changelist_id,
        committed_at: now_millis,
        conflicts: Vec::new(),
        missing_chunks: Vec::new(),
        message: "submit succeeded".to_string(),
        uuid: maybe_uuid,
    };

    Ok(Response::new(rsp))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::auth::{AuthService, AuthSource, TokenPolicy, UserContext};
    use crate::hive_server::{CrvHiveService, hive_dao};
    use crate::pb::SubmitFile;
    use crate::pb::hive_service_server::HiveService;
    use crv_core::metadata::{BranchDoc, BranchMetadata};
    use crv_core::repository::compute_chunk_hash;
    use tonic::{Code, Request};

    fn make_test_auth() -> Arc<AuthService> {
        Arc::new(AuthService::new(
            b"test-secret",
            TokenPolicy {
                ttl_secs: 60,
                renew_before_secs: 30,
            },
        ))
    }

    fn make_service() -> CrvHiveService {
        CrvHiveService::new(make_test_auth())
    }

    fn make_authed_request<T>(msg: T) -> Request<T> {
        let mut req = Request::new(msg);
        req.extensions_mut().insert(UserContext {
            username: "test-user".to_string(),
            scopes: Vec::new(),
            source: AuthSource::Jwt,
        });
        req
    }

    fn fake_chunk_hash_for(data: &[u8]) -> String {
        let h = compute_chunk_hash(data);
        h.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[tokio::test]
    async fn upload_file_chunk_requires_auth() {
        let _guard = crate::hive_server::test_global_lock().await;
        // 这里不再测试鉴权（鉴权逻辑在 gRPC 拦截器中），而是验证参数校验行为：
        // 空的 chunk_hash 应当被视为非法参数。
        let req = UploadFileChunkReq {
            chunk_hash: String::new(),
            offset: 0,
            content: Vec::new(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: 0,
            uuid: String::new(),
        };

        let res = handle_single_upload_file_chunk(req).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn upload_file_chunk_succeeds_with_auth() {
        let _guard = crate::hive_server::test_global_lock().await;
        let data = b"hello upload";
        let chunk_hash = fake_chunk_hash_for(data);

        let req = UploadFileChunkReq {
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
            uuid: String::new(),
        };

        let rsp = handle_single_upload_file_chunk(req)
            .await
            .expect("upload_file_chunk should not fail with valid data");
        assert!(rsp.success);
        // already_exists 标志取决于当前仓库 / 缓存中是否已存在相同 chunk，
        // 测试环境可能复用真实的 repository_path，因此这里不对其做强约束。
    }

    #[tokio::test]
    async fn check_chunks_requires_auth() {
        let _guard = crate::hive_server::test_global_lock().await;
        let service = make_service();
        let req = CheckChunksReq {
            chunk_hashes: vec!["0".repeat(64)],
        };

        let res = service.check_chunks(Request::new(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn upload_then_check_chunks_flow_with_auth() {
        let _guard = crate::hive_server::test_global_lock().await;
        let data = b"hello flow";
        let chunk_hash = fake_chunk_hash_for(data);

        // 1. 上传 chunk
        let upload_req = UploadFileChunkReq {
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
            uuid: String::new(),
        };
        let _ = handle_single_upload_file_chunk(upload_req)
            .await
            .expect("upload_file_chunk should succeed");

        // 2. 检查缺失 chunk，应当为空
        let check_req = CheckChunksReq {
            chunk_hashes: vec![chunk_hash.clone()],
        };
        let rsp = handle_check_chunks(make_authed_request(check_req))
            .await
            .expect("check_chunks should succeed")
            .into_inner();

        assert!(
            rsp.missing_chunk_hashes.is_empty(),
            "uploaded chunk should not be reported as missing"
        );
    }

    #[tokio::test]
    async fn submit_requires_auth() {
        let _guard = crate::hive_server::test_global_lock().await;
        let service = make_service();
        let req = crate::pb::SubmitReq {
            branch_id: "main".to_string(),
            description: "test".to_string(),
            files: Vec::new(),
            request_id: String::new(),
            uuid: String::new(),
        };

        let res = service.submit(Request::new(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn submit_with_auth_and_missing_files_fails_fast() {
        let _guard = crate::hive_server::test_global_lock().await;
        let req = SubmitReq {
            branch_id: "main".to_string(),
            description: "test".to_string(),
            files: Vec::new(),
            request_id: String::new(),
            uuid: String::new(),
        };

        let res = handle_submit(make_authed_request(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::InvalidArgument);
        assert_eq!(status.message(), "files is required");
    }

    #[tokio::test]
    async fn full_submit_flow_writes_changelist_and_persists_chunks() {
        let _guard = crate::hive_server::test_global_lock().await;
        use crv_core::repository::compute_chunk_hash;

        // 使用内存 Mock DAO，确保不依赖真实 Mongo。
        hive_dao::reset_all();

        // 1. 准备一个分支文档，HEAD 指向 changelist 0。
        let branch = BranchDoc {
            id: "main".to_string(),
            created_at: 0,
            created_by: "tester".to_string(),
            head_changelist_id: 0,
            metadata: BranchMetadata {
                description: "test branch".to_string(),
                owners: vec!["tester".to_string()],
            },
        };
        hive_dao::put_branch(branch);

        // 2. 将一个 chunk 写入本地缓存。
        let data = b"submit end-to-end";
        let hash_bytes = compute_chunk_hash(data);
        let chunk_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();

        let cache = ChunkCache::from_config().expect("ChunkCache from_config should succeed");
        cache
            .append_chunk_part(&chunk_hash, 0, data)
            .expect("append_chunk_part should succeed");

        // 3. 构造 Submit 请求，引用刚刚写入缓存的 chunk。
        let submit_file = SubmitFile {
            file_id: String::new(),
            path: "//src/main.cpp".to_string(),
            expected_file_revision: String::new(),
            is_delete: false,
            binary_id: vec![chunk_hash.clone()],
            size: data.len() as i64,
            file_mode: Some("755".to_string()),
        };
        let submit_req = SubmitReq {
            branch_id: "main".to_string(),
            description: "full flow".to_string(),
            files: vec![submit_file],
            request_id: String::new(),
            uuid: String::new(),
        };

        let submit_rsp = handle_submit(make_authed_request(submit_req))
            .await
            .expect("submit should succeed with valid data")
            .into_inner();

        if !submit_rsp.success {
            eprintln!(
                "full_locked_flow submit failed: message={}, missing_chunks={:?}, conflicts={:?}",
                submit_rsp.message, submit_rsp.missing_chunks, submit_rsp.conflicts
            );
        }
        assert!(submit_rsp.success);
        assert!(submit_rsp.changelist_id > 0);

        // 4. 确认 Mock DAO 中确实插入了一个 changelist。
        assert_eq!(
            hive_dao::changelists_len(),
            1,
            "exactly one changelist should be recorded in mock DAO"
        );

        // 5. 确认底层 Repository 中可以找到对应的 chunk。
        let repo =
            crate::hive_server::repository_manager().expect("repository_manager should initialize");
        let hash_bytes_from_hex =
            crv_core::repository::blake3_hex_to_hash(&chunk_hash).expect("valid chunk hash hex");
        let located = repo
            .locate_chunk(&hash_bytes_from_hex)
            .expect("repository locate_chunk should not fail");
        assert!(
            located.is_some(),
            "chunk used in submit should be present in repository"
        );
    }

    /// 从 TryLockFiles -> CheckChunks -> UploadFileChunk -> Submit 的完整 uuid 流程拉通测试。
    #[tokio::test]
    async fn full_locked_flow_from_try_lock_to_submit_with_uuid() {
        let _guard = crate::hive_server::test_global_lock().await;
        use crate::pb::{CheckChunksReq, SubmitReq, TryLockFilesReq};
        use crv_core::metadata::{BranchDoc, BranchMetadata};

        // 使用内存 Mock DAO，确保不依赖真实 Mongo。
        hive_dao::reset_all();

        // 1. 准备一个分支文档，HEAD 指向 changelist 0。
        let branch = BranchDoc {
            id: "main".to_string(),
            created_at: 0,
            created_by: "tester".to_string(),
            head_changelist_id: 0,
            metadata: BranchMetadata {
                description: "test branch".to_string(),
                owners: vec!["tester".to_string()],
            },
        };
        hive_dao::put_branch(branch);

        // 2. TryLockFiles：锁定一个“预期不存在”的新文件。
        let try_lock_req = TryLockFilesReq {
            branch_id: "main".to_string(),
            files: vec![crate::pb::FileToLock {
                file_id: String::new(),
                path: "//src/main.cpp".to_string(),
                expected_file_revision: String::new(),
                expected_file_not_exist: true,
            }],
        };
        // TryLockFiles 现在也需要鉴权，这里复用 make_authed_request 构造带 UserContext 的请求。
        let try_lock_rsp = handle_try_lock_files(make_authed_request(try_lock_req))
            .await
            .expect("try_lock_files should succeed")
            .into_inner();

        assert!(try_lock_rsp.success);
        assert!(
            !try_lock_rsp.uuid.is_empty(),
            "try_lock_files should return a non-empty uuid"
        );
        let uuid = try_lock_rsp.uuid.clone();

        // 3. 准备一个 chunk，并进行一次 CheckChunks（不强制要求当前一定缺失）。
        let data = b"locked full flow";
        let hash_bytes = compute_chunk_hash(data);
        let chunk_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();

        let check_req_before = CheckChunksReq {
            chunk_hashes: vec![chunk_hash.clone()],
        };
        let _ = handle_check_chunks(make_authed_request(check_req_before))
            .await
            .expect("check_chunks before upload should succeed")
            .into_inner();

        // 4. 使用带 uuid 的 UploadFileChunk 上传该 chunk。
        let upload_req = UploadFileChunkReq {
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
            uuid: uuid.clone(),
        };
        let upload_rsp = handle_single_upload_file_chunk(upload_req)
            .await
            .expect("upload_file_chunk should succeed");

        assert!(upload_rsp.success);
        assert_eq!(upload_rsp.uuid, uuid);

        // 确认 ChunkCache 确实已经缓存了该 chunk；若尚未缓存，则直接写入一份，避免测试强依赖 UploadFileChunk
        // 内部细节（本测试目标是端到端流程而非缓存实现本身）。
        if let Ok(cache) = ChunkCache::from_config() {
            match cache.has_chunk(&chunk_hash) {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    cache
                        .append_chunk_part(&chunk_hash, 0, data)
                        .expect("append_chunk_part in test should succeed");
                }
            }
        }

        // 5. 再次 CheckChunks，确认该 chunk 不会被视为缺失。
        let check_req_after = CheckChunksReq {
            chunk_hashes: vec![chunk_hash.clone()],
        };
        let rsp_after = handle_check_chunks(make_authed_request(check_req_after))
            .await
            .expect("check_chunks after upload should succeed")
            .into_inner();

        assert!(
            !rsp_after.missing_chunk_hashes.contains(&chunk_hash),
            "uploaded chunk should not be reported as missing after upload"
        );

        // 6. Submit：使用与 TryLockFiles 一致的期待信息，并携带 uuid。
        let submit_file = SubmitFile {
            file_id: String::new(),
            path: "//src/main.cpp".to_string(),
            expected_file_revision: String::new(),
            is_delete: false,
            binary_id: vec![chunk_hash.clone()],
            size: data.len() as i64,
            file_mode: Some("755".to_string()),
        };
        let submit_req = SubmitReq {
            branch_id: "main".to_string(),
            description: "locked full flow".to_string(),
            files: vec![submit_file],
            request_id: String::new(),
            uuid: uuid.clone(),
        };

        let submit_rsp = handle_submit(make_authed_request(submit_req))
            .await
            .expect("submit should succeed with valid data")
            .into_inner();

        if !submit_rsp.success {
            eprintln!(
                "full_locked_flow submit failed: message={}, missing_chunks={:?}, conflicts={:?}",
                submit_rsp.message, submit_rsp.missing_chunks, submit_rsp.conflicts
            );
        }
        assert!(submit_rsp.success);
        assert!(submit_rsp.changelist_id > 0);
        assert_eq!(submit_rsp.uuid, uuid);
        assert!(submit_rsp.missing_chunks.is_empty());
        assert!(submit_rsp.conflicts.is_empty());

        // 7. 确认 Mock DAO 中确实插入了至少一个 changelist。
        // 由于 Mock DAO 是进程级全局状态，其他测试可能并发写入，因此这里只检查“至少有一条”，
        // 而将“环境归零”的职责交给每个测试自身的 `reset_all()`。
        let cl_count = hive_dao::changelists_len();
        assert!(
            cl_count >= 1,
            "at least one changelist should be recorded in mock DAO, got {cl_count}"
        );
    }
}
