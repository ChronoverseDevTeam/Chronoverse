use crate::auth::{require_user, AuthInterceptor, AuthService};
use crate::config::holder::get_or_init_config;
use crate::database;
use crate::pb::{
    hive_service_server::{HiveService, HiveServiceServer},
    BonjourReq, BonjourRsp, CheckChunksReq, CheckChunksRsp, LoginReq, LoginRsp, SubmitReq,
    SubmitRsp, UploadFileChunkReq, UploadFileChunkRsp,
};
use crv_core::metadata::{
    BranchDoc, ChangelistAction, ChangelistChange, ChangelistDoc, ChangelistMetadata, FileDoc,
    FileMetadata, FileRevisionDoc, FileRevisionMetadata,
};
use crv_core::repository::{Compression, RepositoryError, RepositoryManager, Result as RepoResult};
use mongodb::bson::doc;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tonic::{transport::Server, Request, Response, Status};
use tokio::sync::Mutex;

/// 全局仓库管理器（内容寻址二进制存储）
static REPOSITORY_MANAGER: OnceLock<Arc<RepositoryManager>> = OnceLock::new();

fn get_repository_manager() -> Arc<RepositoryManager> {
    let cfg = get_or_init_config().clone();
    REPOSITORY_MANAGER
        .get_or_init(|| {
            let mgr =
                RepositoryManager::new(&cfg.repository_path).expect("failed to init repository");
            Arc::new(mgr)
        })
        .clone()
}

/// 正在接收的 chunk 缓冲区：chunk_hash -> 已接收的数据
static CHUNK_BUFFERS: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

fn chunk_buffers() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    CHUNK_BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hex_to_chunk_hash(hex: &str) -> Result<crv_core::repository::ChunkHash, Status> {
    let hex = hex.trim();
    if hex.len() != crv_core::repository::HASH_SIZE * 2 {
        return Err(Status::invalid_argument("invalid chunk_hash length"));
    }
    let mut bytes = [0u8; crv_core::repository::HASH_SIZE];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).map_err(|_| Status::invalid_argument("invalid hex"))?;
        bytes[i] = u8::from_str_radix(s, 16)
            .map_err(|_| Status::invalid_argument("invalid hex digits in chunk_hash"))?;
    }
    Ok(bytes)
}

fn chunk_hash_to_hex(hash: &crv_core::repository::ChunkHash) -> String {
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

fn derive_file_id_from_path(path: &str) -> String {
    let hash = blake3::hash(path.as_bytes());
    hash.to_hex().to_string()
}

fn derive_file_revision_id(branch_id: &str, file_id: &str, changelist_id: i64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(branch_id.as_bytes());
    hasher.update(b"|");
    hasher.update(file_id.as_bytes());
    hasher.update(b"|");
    hasher.update(&changelist_id.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

pub struct CrvHiveService {
    auth: Arc<AuthService>,
}

impl CrvHiveService {
    pub fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

#[tonic::async_trait]
impl HiveService for CrvHiveService {
    async fn bonjour(&self, _request: Request<BonjourReq>) -> Result<Response<BonjourRsp>, Status> {
        let rsp = BonjourRsp {
            major_version: 1,
            minor_version: 1,
            api_implementation: "crv-hive".to_string(),
            platform: "rust".to_string(),
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
        };

        Ok(Response::new(rsp))
    }

    async fn login(
        &self,
        request: Request<LoginReq>,
    ) -> Result<Response<LoginRsp>, Status> {
        let req = request.into_inner();

        if req.username.trim().is_empty() || req.password.is_empty() {
            return Err(Status::invalid_argument("username and password are required"));
        }

        // 抽象出的用户名/密码校验逻辑，当前实现总是返回 false，
        // 你可以在后续替换为真实的数据库或其他身份源查询。
        let is_valid = crate::auth::validate_user_credentials(&req.username, &req.password)
            .await
            .map_err(Status::from)?;

        if !is_valid {
            return Err(Status::unauthenticated("invalid username or password"));
        }

        let (token, exp) = self
            .auth
            .issue_token(&req.username, &Vec::new())
            .map_err(Status::from)?;

        let rsp = LoginRsp {
            access_token: token,
            expires_at: exp,
        };

        Ok(Response::new(rsp))
    }

    async fn check_chunks(
        &self,
        request: Request<CheckChunksReq>,
    ) -> Result<Response<CheckChunksRsp>, Status> {
        let req = request.into_inner();
        let repo = get_repository_manager();

        let mut missing = Vec::new();

        for hash_str in req.chunk_hashes {
            match hex_to_chunk_hash(&hash_str) {
                Ok(hash) => match repo.read_chunk(&hash) {
                    Ok(_) => {}
                    Err(RepositoryError::ChunkNotFound { .. }) => missing.push(hash_str),
                    Err(e) => {
                        return Err(Status::internal(format!(
                            "repository error: {e}"
                        )))
                    }
                },
                Err(_) => {
                    // 无法解析的 hash，直接视为缺失
                    missing.push(hash_str);
                }
            }
        }

        Ok(Response::new(CheckChunksRsp {
            missing_chunk_hashes: missing,
        }))
    }

    async fn upload_file_chunk(
        &self,
        request: Request<UploadFileChunkReq>,
    ) -> Result<Response<UploadFileChunkRsp>, Status> {
        let req = request.into_inner();

        if req.chunk_hash.trim().is_empty() {
            return Err(Status::invalid_argument("chunk_hash is required"));
        }

        let repo = get_repository_manager();

        // 先尝试追加到内存缓冲
        let buffers = chunk_buffers();
        {
            let mut map = buffers.lock().await;
            let buf = map
                .entry(req.chunk_hash.clone())
                .or_insert_with(Vec::new);

            // 简单校验 offset：要求按顺序追加
            if req.offset < 0 || req.offset as usize != buf.len() {
                return Err(Status::invalid_argument(
                    "offset does not match current buffered size",
                ));
            }

            buf.extend_from_slice(&req.content);

            if !req.eof {
                // 还未结束，本次请求仅缓存数据
                return Ok(Response::new(UploadFileChunkRsp {
                    success: true,
                    message: "chunk part cached".to_string(),
                    already_exists: false,
                }));
            }
        }

        // EOF：取出完整数据，写入仓库
        let data = {
            let mut map = buffers.lock().await;
            let buf = map
                .remove(&req.chunk_hash)
                .unwrap_or_else(|| Vec::new());
            buf
        };

        // 校验 uncompressed_size
        if req.uncompressed_size > 0 && data.len() != req.uncompressed_size as usize {
            return Err(Status::invalid_argument(
                "uncompressed_size does not match received data length",
            ));
        }

        // 校验 chunk_hash 与内容一致
        let computed = crv_core::repository::compute_chunk_hash(&data);
        let computed_hex = chunk_hash_to_hex(&computed);
        if computed_hex != req.chunk_hash {
            return Err(Status::invalid_argument(
                "chunk_hash does not match computed hash of content",
            ));
        }

        // 写入仓库
        let compression = match req.compression.as_str() {
            "lz4" => Compression::Lz4,
            _ => Compression::None,
        };

        let write_result: RepoResult<_> = repo.write_chunk(&data, compression);
        match write_result {
            Ok(_record) => Ok(Response::new(UploadFileChunkRsp {
                success: true,
                message: "chunk stored".to_string(),
                already_exists: false,
            })),
            Err(RepositoryError::DuplicateHash { .. }) => Ok(Response::new(UploadFileChunkRsp {
                success: true,
                message: "chunk already exists".to_string(),
                already_exists: true,
            })),
            Err(e) => Err(Status::internal(format!(
                "repository error while writing chunk: {e}"
            ))),
        }
    }

    async fn submit(
        &self,
        request: Request<SubmitReq>,
    ) -> Result<Response<SubmitRsp>, Status> {
        // 需要登录用户
        let request = request;
        let user_ctx = require_user(&request)?.clone();
        let req = request.into_inner();

        if req.branch_id.trim().is_empty() {
            return Err(Status::invalid_argument("branch_id is required"));
        }
        if req.request_id.trim().is_empty() {
            return Err(Status::invalid_argument("request_id is required"));
        }

        let db = database::get_database()
            .ok_or_else(|| Status::internal("MongoDB is not initialized"))?;

        let branches = db.collection::<BranchDoc>("branches");
        let changelists = db.collection::<ChangelistDoc>("changelists");
        let files_coll = db.collection::<FileDoc>("files");
        let file_revisions = db.collection::<FileRevisionDoc>("fileRevision");

        // 校验分支是否存在
        let branch = branches
            .find_one(doc! { "_id": &req.branch_id })
            .await
            .map_err(|e| Status::internal(format!("mongo error: {e}")))?
            .ok_or_else(|| Status::not_found("branch not found"))?;

        // 为新的 changelist 分配 ID：当前简单实现为文档总数 + 1
        let count = changelists
            .estimated_document_count()
            .await
            .map_err(|e| Status::internal(format!("mongo error: {e}")))?;
        let next_changelist_id = if count == 0 { 1 } else { count as i64 + 1 };

        let mut conflicts = Vec::new();
        let mut missing_chunks = Vec::new();

        let repo = get_repository_manager();

        // 预先检查所有文件：chunk 是否存在 & 版本是否冲突
        for f in &req.files {
            // 解析文件 ID
            let file_id = if !f.file_id.trim().is_empty() {
                f.file_id.clone()
            } else if !f.path.trim().is_empty() {
                derive_file_id_from_path(&f.path)
            } else {
                return Err(Status::invalid_argument(
                    "each file must have either file_id or path",
                ));
            };

            // 检查所有 binary_id 对应的 chunk 是否存在
            if !f.is_delete {
                for bin in &f.binary_id {
                    match hex_to_chunk_hash(bin) {
                        Ok(hash) => match repo.read_chunk(&hash) {
                            Ok(_) => {}
                            Err(RepositoryError::ChunkNotFound { .. }) => {
                                missing_chunks.push(bin.clone());
                            }
                            Err(e) => {
                                return Err(Status::internal(format!(
                                    "repository error while checking chunk: {e}"
                                )))
                            }
                        },
                        Err(_) => missing_chunks.push(bin.clone()),
                    }
                }
            }

            // 冲突检测（乐观锁）：
            if !f.expect_file_revision.trim().is_empty() {
                let latest = file_revisions
                    .find_one(doc! {
                        "branchId": &req.branch_id,
                        "fileId": &file_id,
                    })
                    .await
                    .map_err(|e| Status::internal(format!("mongo error: {e}")))?;

                match latest {
                    Some(doc) => {
                        if doc.id != f.expect_file_revision {
                            conflicts.push(crate::pb::SubmitConflict {
                                file_id: file_id.clone(),
                                expected_revision: f.expect_file_revision.clone(),
                                current_revision: doc.id,
                            });
                        }
                    }
                    None => {
                        // 期望有旧版本，但实际上没有
                        conflicts.push(crate::pb::SubmitConflict {
                            file_id: file_id.clone(),
                            expected_revision: f.expect_file_revision.clone(),
                            current_revision: "".to_string(),
                        });
                    }
                }
            }
        }

        if !missing_chunks.is_empty() {
            return Ok(Response::new(SubmitRsp {
                success: false,
                changelist_id: 0,
                committed_at: 0,
                conflicts: Vec::new(),
                missing_chunks,
                message: "some chunks are missing".to_string(),
            }));
        }

        if !conflicts.is_empty() {
            return Ok(Response::new(SubmitRsp {
                success: false,
                changelist_id: 0,
                committed_at: 0,
                conflicts,
                missing_chunks: Vec::new(),
                message: "file revision conflicts detected".to_string(),
            }));
        }

        // 通过初步校验，开始创建新的 changelist 与 fileRevision
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut changes = Vec::new();

        for f in &req.files {
            let file_id = if !f.file_id.trim().is_empty() {
                f.file_id.clone()
            } else {
                derive_file_id_from_path(&f.path)
            };

            // 确保 FileDoc 存在
            let file_doc_opt = files_coll
                .find_one(doc! { "_id": &file_id })
                .await
                .map_err(|e| Status::internal(format!("mongo error: {e}")))?;

            if file_doc_opt.is_none() {
                let path = if !f.path.trim().is_empty() {
                    f.path.clone()
                } else {
                    // 若无 path，构造一个简单占位路径
                    format!("//unknown/{}", &file_id)
                };
                let new_file = FileDoc {
                    id: file_id.clone(),
                    path,
                    created_at: now_ms,
                    metadata: FileMetadata {
                        first_introduced_by: user_ctx.username.clone(),
                    },
                };
                files_coll
                    .insert_one(new_file)
                    .await
                    .map_err(|e| Status::internal(format!("mongo error: {e}")))?;
            }

            // 为本次变更生成新的 fileRevision
            let new_rev_id = derive_file_revision_id(&req.branch_id, &file_id, next_changelist_id);
            let action = if f.is_delete {
                ChangelistAction::Delete
            } else if f.expect_file_revision.trim().is_empty() {
                ChangelistAction::Create
            } else {
                ChangelistAction::Modify
            };

            let rev_doc = FileRevisionDoc {
                id: new_rev_id.clone(),
                branch_id: req.branch_id.clone(),
                file_id: file_id.clone(),
                changelist_id: next_changelist_id,
                binary_id: if f.is_delete {
                    Vec::new()
                } else {
                    f.binary_id.clone()
                },
                parent_revision_id: f.expect_file_revision.clone(),
                size: f.size,
                is_delete: f.is_delete,
                created_at: now_ms,
                metadata: FileRevisionMetadata {
                    file_mode: f.file_mode.clone().unwrap_or_default(),
                    hash: String::new(),
                    is_binary: false,
                    language: String::new(),
                },
            };

            file_revisions
                .insert_one(rev_doc)
                .await
                .map_err(|e| Status::internal(format!("mongo error: {e}")))?;

            changes.push(ChangelistChange {
                file: file_id,
                action,
                revision: new_rev_id,
            });
        }

        let cl_doc = ChangelistDoc {
            id: next_changelist_id,
            parent_changelist_id: branch.head_changelist_id,
            branch_id: req.branch_id.clone(),
            author: user_ctx.username.clone(),
            description: req.description.clone(),
            changes,
            committed_at: now_ms,
            files_count: req.files.len() as i64,
            metadata: ChangelistMetadata { labels: Vec::new() },
        };

        changelists
            .insert_one(cl_doc)
            .await
            .map_err(|e| Status::internal(format!("mongo error: {e}")))?;

        // 更新 branch 的 HEAD 指向
        branches
            .update_one(doc! { "_id": &req.branch_id },
                        doc! { "$set": { "headChangelistId": next_changelist_id } })
            .await
            .map_err(|e| Status::internal(format!("mongo error: {e}")))?;

        Ok(Response::new(SubmitRsp {
            success: true,
            changelist_id: next_changelist_id,
            committed_at: now_ms,
            conflicts: Vec::new(),
            missing_chunks: Vec::new(),
            message: String::new(),
        }))
    }
}

/// 启动 gRPC 服务器（优雅关闭）
pub async fn start_server_with_shutdown<S>(
    addr: std::net::SocketAddr,
    shutdown: S,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    // 基于全局配置初始化 AuthService，并构建 gRPC 拦截器
    let auth = AuthService::from_config();
    let service = CrvHiveService::new(Arc::clone(&auth));
    let interceptor = AuthInterceptor::new(Arc::clone(&auth));

    Server::builder()
        .add_service(HiveServiceServer::with_interceptor(service, interceptor))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let auth = AuthService::from_config();
    let service = CrvHiveService::new(Arc::clone(&auth));
    let interceptor = AuthInterceptor::new(auth);

    Server::builder()
        .add_service(HiveServiceServer::with_interceptor(service, interceptor))
        .serve(addr)
        .await?;

    Ok(())
}
