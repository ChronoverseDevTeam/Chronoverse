use crate::auth::{AuthService, AuthInterceptor};
use crate::pb::hive_service_server::{HiveService, HiveServiceServer};
use tonic::transport::Server;
use std::sync::Arc;

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
    async fn bonjour(
        &self,
        _request: tonic::Request<crate::pb::BonjourReq>,
    ) -> Result<tonic::Response<crate::pb::BonjourRsp>, tonic::Status> {
        let rsp = crate::pb::BonjourRsp {
            major_version: 1,
            minor_version: 1,
            api_implementation: "crv-hive".to_string(),
            platform: "rust".to_string(),
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
        };

        Ok(tonic::Response::new(rsp))
    }

    async fn login(
        &self,
        request: tonic::Request<crate::pb::LoginReq>,
    ) -> Result<tonic::Response<crate::pb::LoginRsp>, tonic::Status> {
        let req = request.into_inner();

        if req.username.trim().is_empty() || req.password.is_empty() {
            return Err(tonic::Status::invalid_argument(
                "username and password are required",
            ));
        }

        // 抽象出的用户名/密码校验逻辑，当前实现总是返回 false，
        // 你可以在后续替换为真实的数据库或其他身份源查询。
        let is_valid = crate::auth::validate_user_credentials(&req.username, &req.password)
            .await
            .map_err(tonic::Status::from)?;

        if !is_valid {
            return Err(tonic::Status::unauthenticated(
                "invalid username or password",
            ));
        }

        let (token, exp) = self
            .auth
            .issue_token(&req.username, &Vec::new())
            .map_err(tonic::Status::from)?;

        let rsp = crate::pb::LoginRsp {
            access_token: token,
            expires_at: exp,
        };

        Ok(tonic::Response::new(rsp))
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
