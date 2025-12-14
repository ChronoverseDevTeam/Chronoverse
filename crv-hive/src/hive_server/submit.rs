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
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::{Request, Response, Status};

/// 预检查一批文件是否可以被当前 changelist 锁定。
pub async fn handle_try_lock_files(
    request: Request<TryLockFilesReq>,
) -> Result<Response<TryLockFilesResp>, Status> {
    use crate::hive_server::hive_dao as dao;

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
        };
        return Ok(Response::new(rsp));
    }

    // 全部成功加锁。
    let rsp = TryLockFilesResp {
        success: true,
        file_unable_to_lock: Vec::new(),
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

/// 上传文件内容块到服务器进行缓存。
pub async fn handle_upload_file_chunk(
    request: Request<UploadFileChunkReq>,
) -> Result<Response<UploadFileChunkRsp>, Status> {
    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let _user = auth::require_user(&request)?;

    let req = request.into_inner();

    let chunk_hash = req.chunk_hash.trim().to_lowercase();
    if chunk_hash.is_empty() {
        return Err(Status::invalid_argument("chunk_hash is required"));
    }

    let offset = if req.offset < 0 {
        return Err(Status::invalid_argument("offset must be non-negative"));
    } else {
        req.offset as u64
    };

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
                            message: "chunk already exists in repository".to_string(),
                            already_exists: true,
                        };
                        return Ok(Response::new(rsp));
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
                    message: "chunk already exists in cache".to_string(),
                    already_exists: true,
                };
                return Ok(Response::new(rsp));
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
        message: String::new(),
        already_exists: false,
    };
    Ok(Response::new(rsp))
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

    let branch_id = req.branch_id.trim();
    if branch_id.is_empty() {
        return Err(Status::invalid_argument("branch_id is required"));
    }
    if req.files.is_empty() {
        return Err(Status::invalid_argument("files is required"));
    }

    // 加载分支信息，获取当前 HEAD changelist。
    let branch = dao::find_branch_by_id(branch_id)
        .await
        .map_err(|e| Status::internal(format!("database error while reading branch: {e}")))?
        .ok_or_else(|| Status::not_found("branch not found"))?;
    let parent_changelist_id = branch.head_changelist_id;

    // 计算新的 changelist ID（简单自增）。
    let max_id = dao::get_max_changelist_id()
        .await
        .map_err(|e| Status::internal(format!("database error while reading changelist: {e}")))?;
    let new_changelist_id = max_id + 1;

    // 当前时间戳（毫秒）
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

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
        let rsp = SubmitRsp {
            success: false,
            changelist_id: 0,
            committed_at: 0,
            conflicts: Vec::new(),
            missing_chunks,
            message: "missing chunks, please upload them before submit".to_string(),
        };
        return Ok(Response::new(rsp));
    }

    // 将本次提交涉及到的所有 chunk 从缓存写入 Repository（若尚未存在）。
    let mut used_chunk_hashes: HashSet<String> = HashSet::new();
    for f in &req.files {
        for ch in &f.binary_id {
            let ch_trim = ch.trim().to_string();
            if ch_trim.is_empty() {
                continue;
            }
            used_chunk_hashes.insert(ch_trim.to_lowercase());
        }
    }

    {
        let cache = ChunkCache::from_config().map_err(|e| {
            Status::internal(format!(
                "failed to initialize chunk cache for repository write: {e}"
            ))
        })?;

        for ch in &used_chunk_hashes {
            // 从缓存中读取完整 chunk 内容。
            let bytes = cache
                .read_chunk(ch)
                .map_err(|e| Status::internal(format!("failed to read cached chunk {ch}: {e}")))?;

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

    // 如果存在冲突，则直接返回，不进行任何写入。
    if !conflicts.is_empty() {
        let rsp = SubmitRsp {
            success: false,
            changelist_id: 0,
            committed_at: 0,
            conflicts,
            missing_chunks: Vec::new(),
            message: "submit aborted due to file revision conflicts".to_string(),
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

    let rsp = SubmitRsp {
        success: true,
        changelist_id: new_changelist_id,
        committed_at: now_millis,
        conflicts: Vec::new(),
        missing_chunks: Vec::new(),
        message: "submit succeeded".to_string(),
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
        let service = make_service();
        let req = UploadFileChunkReq {
            chunk_hash: "0".repeat(64),
            offset: 0,
            content: Vec::new(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: 0,
        };

        let res = service.upload_file_chunk(Request::new(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn upload_file_chunk_succeeds_with_auth() {
        let service = make_service();
        let data = b"hello upload";
        let chunk_hash = fake_chunk_hash_for(data);

        let req = UploadFileChunkReq {
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        };

        let rsp = service
            .upload_file_chunk(make_authed_request(req))
            .await
            .expect("upload_file_chunk should not fail with auth")
            .into_inner();
        assert!(rsp.success);
        // already_exists 标志取决于当前仓库 / 缓存中是否已存在相同 chunk，
        // 测试环境可能复用真实的 repository_path，因此这里不对其做强约束。
    }

    #[tokio::test]
    async fn check_chunks_requires_auth() {
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
        let service = make_service();
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
        };
        let _ = service
            .upload_file_chunk(make_authed_request(upload_req))
            .await
            .expect("upload_file_chunk should succeed")
            .into_inner();

        // 2. 检查缺失 chunk，应当为空
        let check_req = CheckChunksReq {
            chunk_hashes: vec![chunk_hash.clone()],
        };
        let rsp = service
            .check_chunks(make_authed_request(check_req))
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
        let service = make_service();
        let req = crate::pb::SubmitReq {
            branch_id: "main".to_string(),
            description: "test".to_string(),
            files: Vec::new(),
            request_id: String::new(),
        };

        let res = service.submit(Request::new(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn submit_with_auth_and_missing_files_fails_fast() {
        let req = SubmitReq {
            branch_id: "main".to_string(),
            description: "test".to_string(),
            files: Vec::new(),
            request_id: String::new(),
        };

        let res = handle_submit(make_authed_request(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::InvalidArgument);
        assert_eq!(status.message(), "files is required");
    }

    #[tokio::test]
    async fn full_submit_flow_writes_changelist_and_persists_chunks() {
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
        };

        let submit_rsp = handle_submit(make_authed_request(submit_req))
            .await
            .expect("submit should succeed with valid data")
            .into_inner();

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
}
