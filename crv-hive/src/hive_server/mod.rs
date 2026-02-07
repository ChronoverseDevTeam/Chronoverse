use crate::auth::{AuthInterceptor, AuthService};
use crate::hive_server::fetch::download;
use crate::logging::HiveLog;
use crate::pb::{
    BonjourReq, BonjourRsp, CheckChunksReq, CheckChunksRsp, DownloadFileChunkReq,
    GetFileTreeReq, GetFileTreeRsp, LaunchSubmitReq, LaunchSubmitRsp, LoginReq, LoginRsp, RegisterReq,
    RegisterRsp, SubmitReq, SubmitRsp, UploadFileChunkReq,
    hive_service_server::{HiveService, HiveServiceServer},
};
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHasher};
use http::header::{HeaderName, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use http::Method;
use crv_core::repository::{
    Repository
};
use rand::rngs::OsRng;
use std::sync::{Arc, OnceLock};
use tonic::{Request, Response, Status, transport::Server};
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};

mod fetch;
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

#[tonic::async_trait]
impl HiveService for CrvHiveService {
    async fn bonjour(&self, _request: Request<BonjourReq>) -> Result<Response<BonjourRsp>, Status> {
        let log = HiveLog::from_request("Bonjour", &_request);
        let _g = log.enter();
        log.info("rpc start");
        let rsp = BonjourRsp {
            major_version: 1,
            minor_version: 1,
            api_implementation: "crv-hive".to_string(),
            platform: "rust".to_string(),
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
        };
        let out = Ok(Response::new(rsp));
        log.finish_ok();
        out
    }

    async fn login(&self, request: Request<LoginReq>) -> Result<Response<LoginRsp>, Status> {
        let log = HiveLog::from_request("Login", &request);
        let _g = log.enter();
        log.info("rpc start");
        let req = request.into_inner();
        log.debug(&format!("login attempt username={}", req.username));

        if req.username.trim().is_empty() || req.password.is_empty() {
            let e = Status::invalid_argument(
                "username and password are required",
            );
            log.finish_err(&e);
            return Err(e);
        }

        // 抽象出的用户名/密码校验逻辑，当前实现总是返回 false，
        // 你可以在后续替换为真实的数据库或其他身份源查询。
        let is_valid = crate::auth::validate_user_credentials(&req.username, &req.password)
            .await
            .map_err(Status::from)?;

        if !is_valid {
            let e = Status::unauthenticated("invalid username or password");
            log.finish_err(&e);
            return Err(e);
        }

        let (token, exp) = self
            .auth
            .issue_token(&req.username, &Vec::new())
            .map_err(Status::from)?;

        let rsp = LoginRsp {
            access_token: token,
            expires_at: exp,
        };

        let out = Ok(Response::new(rsp));
        log.finish_ok();
        out
    }

    async fn register(
        &self,
        request: Request<RegisterReq>,
    ) -> Result<Response<RegisterRsp>, Status> {
        let log = HiveLog::from_request("Register", &request);
        let _g = log.enter();
        log.info("rpc start");
        let req = request.into_inner();

        let username = req.username.trim();
        let password = req.password;

        if username.is_empty() || password.is_empty() {
            let e = Status::invalid_argument(
                "username and password are required",
            );
            log.finish_err(&e);
            return Err(e);
        }

        if username.len() < 3 {
            let e = Status::invalid_argument(
                "username must be at least 3 characters",
            );
            log.finish_err(&e);
            return Err(e);
        }

        if password.len() < 6 {
            let e = Status::invalid_argument(
                "password must be at least 6 characters",
            );
            log.finish_err(&e);
            return Err(e);
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
                let s = Status::internal(format!(
                    "database error while checking user: {e}"
                ));
                log.finish_err(&s);
                return Err(s);
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
            let s = Status::internal(format!(
                "database error while inserting user: {e}"
            ));
            log.finish_err(&s);
            return Err(s);
        }

        let out = Ok(Response::new(RegisterRsp {
            success: true,
            message: String::from("registered"),
        }));
        log.finish_ok();
        out
    }


    type DownloadFileChunkStream = download::DownloadFileChunkStream;
    type UploadFileChunkStream = submit::submit::UploadFileChunkStream;

    async fn download_file_chunk(
        &self,
        request: Request<DownloadFileChunkReq>,
    ) -> Result<Response<Self::DownloadFileChunkStream>, Status> {
        let log = HiveLog::from_request("DownloadFileChunk", &request);
        let _g = log.enter();
        log.info("rpc start");
        let out = download::handle_download_file_chunk(log.clone(), request).await;
        match &out {
            Ok(_) => log.info("rpc accepted (stream opened)"),
            Err(e) => log.finish_err(e),
        }
        out
    }

    async fn launch_submit(
        &self,
        request: Request<LaunchSubmitReq>,
    ) -> Result<Response<LaunchSubmitRsp>, Status> {
        let log = HiveLog::from_request("LaunchSubmit", &request);
        let _g = log.enter();
        log.info("rpc start");
        let out = submit::launch_submit::handle_launch_submit(log.clone(), request).await;
        match &out {
            Ok(_) => log.finish_ok(),
            Err(e) => log.finish_err(e),
        }
        out
    }

    async fn check_chunks(
        &self,
        request: Request<CheckChunksReq>,
    ) -> Result<Response<CheckChunksRsp>, Status> {
        let log = HiveLog::from_request("CheckChunks", &request);
        let _g = log.enter();
        log.info("rpc start");
        let _req = request.into_inner();
        let rsp = CheckChunksRsp {
            missing_chunk_hashes: _req.chunk_hashes,
        };
        let out = Ok(Response::new(rsp));
        log.finish_ok();
        out
    }

    async fn upload_file_chunk(
        &self,
        _request: Request<tonic::Streaming<UploadFileChunkReq>>,
    ) -> Result<Response<Self::UploadFileChunkStream>, Status> {
        let log = HiveLog::from_request("UploadFileChunk", &_request);
        let _g = log.enter();
        log.info("rpc start (stream)");
        let out = submit::upload_file_chunk::upload_file_chunk(log.clone(), _request);
        match &out {
            Ok(_) => log.info("rpc accepted (stream opened)"),
            Err(e) => log.finish_err(e),
        }
        out
    }

    async fn submit(
        &self,
        _request: Request<SubmitReq>,
    ) -> Result<Response<SubmitRsp>, Status> {
        let log = HiveLog::from_request("Submit", &_request);
        let _g = log.enter();
        log.info("rpc start");
        let out = submit::submit::submit(log.clone(), _request).await;
        match &out {
            Ok(_) => log.finish_ok(),
            Err(e) => log.finish_err(e),
        }
        out
    }

    async fn get_file_tree(
        &self,
        request: Request<GetFileTreeReq>,
    ) -> Result<Response<GetFileTreeRsp>, Status> {
        let log = HiveLog::from_request("GetFileTree", &request);
        let _g = log.enter();
        log.info("rpc start");
        let out = fetch::get_file_tree::get_file_tree(log.clone(), request).await;
        match &out {
            Ok(_) => log.finish_ok(),
            Err(e) => log.finish_err(e),
        }
        out
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
