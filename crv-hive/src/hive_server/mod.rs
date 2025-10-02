use tonic::{transport::Server, Request, Response, Status};
pub mod auth;
use crate::hive_server::auth::check_auth;
use crate::pb::hive_service_server::{HiveService, HiveServiceServer};
use crate::pb::{CreateWorkspaceReq, GreetingReq, ListWorkspaceReq, ListWorkspaceRsp, NilRsp, LoginReq, LoginRsp, RegisterReq, RegisterRsp, CreateTokenReq, CreateTokenRsp, ListTokensReq, ListTokensRsp, RevokeTokenReq, RevokeTokenRsp};
use crate::hive_server::auth::{RenewToken, apply_renew_metadata};

#[derive(Default)]
pub struct CrvHiveService;

#[tonic::async_trait]
impl HiveService for CrvHiveService {
    async fn greeting(
        &self,
        request: Request<GreetingReq>,
    ) -> Result<Response<NilRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = crate::logic::create_workspace::greeting(request).await?;
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceReq>,
    ) -> Result<Response<NilRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = Response::new(NilRsp {});
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn list_workspaces(
        &self,
        request: Request<ListWorkspaceReq>,
    ) -> Result<Response<ListWorkspaceRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = crate::logic::list_workspaces::list_workspaces(request).await?;
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn login(
        &self,
        request: Request<LoginReq>,
    ) -> Result<Response<LoginRsp>, Status> {
        crate::logic::auth::login(request).await
    }

    async fn register(
        &self,
        request: Request<RegisterReq>,
    ) -> Result<Response<RegisterRsp>, Status> {
        crate::logic::auth::register(request).await
    }

    async fn create_token(
        &self,
        request: Request<CreateTokenReq>,
    ) -> Result<Response<CreateTokenRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = crate::logic::tokens::create_token(request).await?;
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn list_tokens(
        &self,
        request: Request<ListTokensReq>,
    ) -> Result<Response<ListTokensRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = crate::logic::tokens::list_tokens(request).await?;
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn revoke_token(
        &self,
        request: Request<RevokeTokenReq>,
    ) -> Result<Response<RevokeTokenRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = crate::logic::tokens::revoke_token(request).await?;
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
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
    let service = CrvHiveService::default();

    Server::builder()
        .add_service(HiveServiceServer::with_interceptor(service, check_auth))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let greeter = CrvHiveService::default();

    Server::builder()
        .add_service(HiveServiceServer::with_interceptor(greeter, check_auth))
        .serve(addr)
        .await?;

    Ok(())
}
