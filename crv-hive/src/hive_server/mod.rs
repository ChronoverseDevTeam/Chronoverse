use crate::auth::{AuthInterceptor, AuthService};
use crate::pb::{
    hive_service_server::{HiveService, HiveServiceServer},
    BonjourReq, BonjourRsp, CheckChunksReq, CheckChunksRsp, LoginReq, LoginRsp, SubmitReq,
    SubmitRsp, TryLockFilesReq, TryLockFilesResp, UploadFileChunkReq, UploadFileChunkRsp,
};
use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};

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

    /// 预检查一批文件是否可以被当前 changelist 锁定（基于 expect_file_revision 等条件）
    /// 仅声明签名，具体实现由后续补充
    async fn try_lock_files(
        &self,
        _request: Request<TryLockFilesReq>,
    ) -> Result<Response<TryLockFilesResp>, Status> {
        Err(Status::unimplemented("TryLockFiles is not implemented yet"))
    }

    /// 检查服务器端当前缺少哪些 chunk_hash
    /// 仅声明签名，具体实现由后续补充
    async fn check_chunks(
        &self,
        _request: Request<CheckChunksReq>,
    ) -> Result<Response<CheckChunksRsp>, Status> {
        Err(Status::unimplemented("CheckChunks is not implemented yet"))
    }

    /// 上传文件内容块到服务器进行缓存
    /// 仅声明签名，具体实现由后续补充
    async fn upload_file_chunk(
        &self,
        _request: Request<UploadFileChunkReq>,
    ) -> Result<Response<UploadFileChunkRsp>, Status> {
        Err(Status::unimplemented("UploadFileChunk is not implemented yet"))
    }

    /// 提交一个新的 changelist，使用之前上传好的文件块作为数据源
    /// 仅声明签名，具体实现由后续补充
    async fn submit(
        &self,
        _request: Request<SubmitReq>,
    ) -> Result<Response<SubmitRsp>, Status> {
        Err(Status::unimplemented("Submit is not implemented yet"))
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
