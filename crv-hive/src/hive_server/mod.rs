use crate::auth::{AuthInterceptor, AuthService};
use crate::pb::{
    BonjourReq, BonjourRsp, CheckChunksReq, CheckChunksRsp, DownloadFileChunkReq,
    GetFileTreeReq, GetFileTreeRsp, LaunchSubmitReq, LaunchSubmitRsp, LoginReq, LoginRsp, RegisterReq,
    RegisterRsp, SubmitReq, SubmitRsp, UploadFileChunkReq, UploadFileChunkRsp,
    hive_service_server::{HiveService, HiveServiceServer},
};
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHasher};
use http::header::{HeaderName, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use http::Method;
use crv_core::repository::{
    Repository, blake3_hash_to_hex, compute_blake3_str,
};
use crv_core::tree::depot_tree::DepotTree;
use rand::rngs::OsRng;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status, transport::Server};
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};

mod fetch;
mod download;
mod submit;

pub struct CrvHiveService {
    auth: Arc<AuthService>,
}

impl CrvHiveService {
    pub fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

use crate::config::holder::get_or_init_config;

/// 全局 Submit 串行化锁。
///
/// 当前实现为进程级别的单一互斥锁，保证在同一 hive 实例内所有 Submit 调用串行执行，
/// 从而避免并发提交导致的 HEAD 竞争更新等问题。
static SUBMIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn submit_lock() -> &'static Mutex<()> {
    SUBMIT_LOCK.get_or_init(|| Mutex::new(()))
}

/// 全局 DepotTree 实例，用于在内存中维护分支的文件锁与文件树缓存。
static DEPOT_TREE: OnceLock<Mutex<DepotTree>> = OnceLock::new();

pub(crate) fn depot_tree() -> &'static Mutex<DepotTree> {
    DEPOT_TREE.get_or_init(|| Mutex::new(DepotTree::new()))
}

/// 全局 RepositoryManager 实例，用于访问底层 chunk 仓库。
///
/// 使用 OnceLock 包裹 Result，以便在初始化失败时将错误信息缓存下来并转换为 gRPC Status 返回。
static REPOSITORY_MANAGER: OnceLock<Result<Repository, String>> = OnceLock::new();

pub(crate) fn repository_manager() -> Result<&'static Repository, Status> {
    let cfg = get_or_init_config();
    let repo_root = cfg.repository_path.clone();

    let res = REPOSITORY_MANAGER.get_or_init(|| {
        Repository::new(&repo_root)
            .map_err(|e| format!("failed to open repository at {repo_root}: {e}"))
    });

    match res {
        Ok(manager) => Ok(manager),
        Err(msg) => Err(Status::internal(msg.clone())),
    }
}

/// 数据访问层抽象：
/// - 在正常构建中，直接复用 `crate::database::dao`；
/// - 在测试构建中，使用内存中的 Mock DAO，避免依赖真实 MongoDB。
#[cfg(not(test))]
pub(crate) mod hive_dao {
    pub use crate::database::dao::{
    };
}

#[cfg(test)]
pub(crate) mod hive_dao {
    use super::*;
    use crate::database::dao::DaoError;
    use crv_core::metadata::{BranchDoc, ChangelistDoc, FileDoc, FileRevisionDoc};
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub type DaoResult<T> = Result<T, DaoError>;

    #[derive(Default)]
    struct MockState {
        branches: HashMap<String, BranchDoc>,
        files: HashMap<String, FileDoc>,
        file_revisions: Vec<FileRevisionDoc>,
        changelists: HashMap<i64, ChangelistDoc>,
        max_changelist_id: i64,
    }

    static STATE: OnceLock<Mutex<MockState>> = OnceLock::new();

    fn state() -> &'static Mutex<MockState> {
        STATE.get_or_init(|| Mutex::new(MockState::default()))
    }

    /// 测试辅助函数：重置全部内存数据。
    pub fn reset_all() {
        let mut s = state().lock().expect("lock mock dao state");
        *s = MockState::default();
    }

    /// 测试辅助函数：插入或更新一个分支文档。
    pub fn put_branch(doc: BranchDoc) {
        let mut s = state().lock().expect("lock mock dao state");
        s.branches.insert(doc.id.clone(), doc);
    }

    /// 测试辅助函数：获取当前内存中的 changelist 文档数量。
    pub fn changelists_len() -> usize {
        let s = state().lock().expect("lock mock dao state");
        s.changelists.len()
    }

    pub async fn find_branch_by_id(branch_id: &str) -> DaoResult<Option<BranchDoc>> {
        let s = state().lock().expect("lock mock dao state");
        Ok(s.branches.get(branch_id).cloned())
    }

    pub async fn find_changelist_by_id(changelist_id: i64) -> DaoResult<Option<ChangelistDoc>> {
        let s = state().lock().expect("lock mock dao state");
        Ok(s.changelists.get(&changelist_id).cloned())
    }

    pub async fn allocate_changelist_id() -> DaoResult<i64> {
        let mut s = state().lock().expect("lock mock dao state");
        s.max_changelist_id += 1;
        Ok(s.max_changelist_id)
    }

    pub async fn find_file_revision_by_branch_file_and_cl(
        branch_id: &str,
        file_id: &str,
        changelist_id: i64,
    ) -> DaoResult<Option<FileRevisionDoc>> {
        let s = state().lock().expect("lock mock dao state");
        Ok(s.file_revisions
            .iter()
            .find(|rev| {
                rev.branch_id == branch_id
                    && rev.file_id == file_id
                    && rev.changelist_id == changelist_id
            })
            .cloned())
    }

    pub async fn find_file_revision_by_id(revision_id: &str) -> DaoResult<Option<FileRevisionDoc>> {
        let s = state().lock().expect("lock mock dao state");
        Ok(s.file_revisions.iter().find(|r| r.id == revision_id).cloned())
    }

    pub async fn find_file_by_id(file_id: &str) -> DaoResult<Option<FileDoc>> {
        let s = state().lock().expect("lock mock dao state");
        Ok(s.files.get(file_id).cloned())
    }

    pub async fn insert_file(doc: FileDoc) -> DaoResult<()> {
        let mut s = state().lock().expect("lock mock dao state");
        s.files.insert(doc.id.clone(), doc);
        Ok(())
    }

    pub async fn insert_file_revisions(docs: Vec<FileRevisionDoc>) -> DaoResult<()> {
        if docs.is_empty() {
            return Ok(());
        }
        let mut s = state().lock().expect("lock mock dao state");
        for d in docs {
            s.file_revisions.push(d);
        }
        Ok(())
    }

    pub async fn insert_changelist(doc: ChangelistDoc) -> DaoResult<()> {
        let mut s = state().lock().expect("lock mock dao state");
        s.max_changelist_id = s.max_changelist_id.max(doc.id);
        s.changelists.insert(doc.id, doc);
        Ok(())
    }

    pub async fn update_branch_head(branch_id: &str, new_head: i64) -> DaoResult<()> {
        let mut s = state().lock().expect("lock mock dao state");
        if let Some(branch) = s.branches.get_mut(branch_id) {
            branch.head_changelist_id = new_head;
        }
        Ok(())
    }
}

pub(crate) fn derive_file_id_from_path(path: &str) -> String {
    let hash_bytes = compute_blake3_str(path);
    blake3_hash_to_hex(&hash_bytes)
}

fn build_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::GET, Method::OPTIONS])
        .allow_headers([
            ACCEPT,
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("grpc-timeout"),
            HeaderName::from_static("x-grpc-web"),
            HeaderName::from_static("x-user-agent"),
            HeaderName::from_static("grpc-encoding"),
            HeaderName::from_static("grpc-accept-encoding"),
        ])
        .expose_headers([
            HeaderName::from_static("grpc-status"),
            HeaderName::from_static("grpc-message"),
            HeaderName::from_static("grpc-status-details-bin"),
        ])
}

/// 测试用全局互斥锁：用于串行化依赖进程级全局状态（Mock DAO / DepotTree 等）的单元测试，
/// 避免 `cargo test` 默认并行执行导致的状态互相污染与偶发失败。
#[cfg(test)]
static TEST_GLOBAL_LOCK: tokio::sync::OnceCell<Mutex<()>> = tokio::sync::OnceCell::const_new();

#[cfg(test)]
pub(crate) async fn test_global_lock() -> tokio::sync::MutexGuard<'static, ()> {
    TEST_GLOBAL_LOCK
        .get_or_init(|| async { Mutex::new(()) })
        .await
        .lock()
        .await
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

    async fn login(&self, request: Request<LoginReq>) -> Result<Response<LoginRsp>, Status> {
        let req = request.into_inner();

        if req.username.trim().is_empty() || req.password.is_empty() {
            return Err(Status::invalid_argument(
                "username and password are required",
            ));
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

    async fn register(
        &self,
        request: Request<RegisterReq>,
    ) -> Result<Response<RegisterRsp>, Status> {
        let req = request.into_inner();

        let username = req.username.trim();
        let password = req.password;

        if username.is_empty() || password.is_empty() {
            return Err(Status::invalid_argument(
                "username and password are required",
            ));
        }

        if username.len() < 3 {
            return Err(Status::invalid_argument(
                "username must be at least 3 characters",
            ));
        }

        if password.len() < 6 {
            return Err(Status::invalid_argument(
                "password must be at least 6 characters",
            ));
        }

        // 检查用户名是否已存在
        match crate::database::dao::find_user_by_username(username).await {
            Ok(Some(_)) => {
                return Ok(Response::new(RegisterRsp {
                    success: false,
                    message: "username already exists".to_string(),
                }));
            }
            Ok(None) => {}
            Err(e) => {
                return Err(Status::internal(format!(
                    "database error while checking user: {e}"
                )));
            }
        }

        // 使用 Argon2 对密码进行哈希
        let salt = SaltString::generate(&mut OsRng);
        let argon = Argon2::default();
        let password_hash = argon
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| Status::internal("failed to hash password"))?
            .to_string();

        if let Err(e) = crate::database::dao::insert_user(username, &password_hash).await {
            return Err(Status::internal(format!(
                "database error while inserting user: {e}"
            )));
        }

        Ok(Response::new(RegisterRsp {
            success: true,
            message: String::from("registered"),
        }))
    }


    type DownloadFileChunkStream = download::DownloadFileChunkStream;
    type UploadFileChunkStream = submit::submit::UploadFileChunkStream;

    async fn download_file_chunk(
        &self,
        request: Request<DownloadFileChunkReq>,
    ) -> Result<Response<Self::DownloadFileChunkStream>, Status> {
        download::handle_download_file_chunk(request).await
    }

    async fn launch_submit(
        &self,
        request: Request<LaunchSubmitReq>,
    ) -> Result<Response<LaunchSubmitRsp>, Status> {
        submit::launch_submit::handle_launch_submit(request).await
    }

    async fn check_chunks(
        &self,
        request: Request<CheckChunksReq>,
    ) -> Result<Response<CheckChunksRsp>, Status> {
        let _req = request.into_inner();
        let rsp = CheckChunksRsp {
            missing_chunk_hashes: vec![],
        };
        Ok(Response::new(rsp))
    }

    async fn upload_file_chunk(
        &self,
        _request: Request<tonic::Streaming<UploadFileChunkReq>>,
    ) -> Result<Response<Self::UploadFileChunkStream>, Status> {
        use tokio::sync::mpsc;
        use tokio_stream::wrappers::ReceiverStream;
        let (_tx, rx) = mpsc::channel::<Result<UploadFileChunkRsp, Status>>(32);
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn submit(
        &self,
        _request: Request<SubmitReq>,
    ) -> Result<Response<SubmitRsp>, Status> {
        let rsp = SubmitRsp {
            success: false,
            changelist_id: 0,
            committed_at: 0,
            conflicts: vec![],
            missing_chunks: vec![],
            latest_revision: std::collections::HashMap::new(),
            message: "not implemented".to_string(),
        };
        Ok(Response::new(rsp))
    }

    async fn get_file_tree(
        &self,
        _request: Request<GetFileTreeReq>,
    ) -> Result<Response<GetFileTreeRsp>, Status> {
        let rsp = GetFileTreeRsp {
            file_tree_root: vec![],
        };
        Ok(Response::new(rsp))
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
    let cors = build_cors_layer();

    Server::builder()
        .accept_http1(true)
        .layer(cors)
        .layer(GrpcWebLayer::new())
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
    let cors = build_cors_layer();

    Server::builder()
        .accept_http1(true)
        .layer(cors)
        .layer(GrpcWebLayer::new())
        .add_service(HiveServiceServer::with_interceptor(service, interceptor))
        .serve(addr)
        .await?;

    Ok(())
}
