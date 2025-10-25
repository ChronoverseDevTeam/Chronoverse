use tonic::{transport::Server, Request, Response, Status};
use crate::pb::hive_service_server::{HiveService, HiveServiceServer};
use crate::pb::{UpsertWorkspaceReq, GreetingReq, ListWorkspaceReq, ListWorkspaceRsp, NilRsp, LoginReq, LoginRsp, RegisterReq, RegisterRsp};
use crate::middleware::{RenewToken, apply_renew_metadata, enforce_jwt};

#[derive(Default)]
pub struct CrvHiveService;

#[tonic::async_trait]
impl HiveService for CrvHiveService {
    async fn greeting(
        &self,
        request: Request<GreetingReq>,
    ) -> Result<Response<NilRsp>, Status> {
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = Response::new(NilRsp {});
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn upsert_workspace(
        &self,
        request: Request<UpsertWorkspaceReq>,
    ) -> Result<Response<NilRsp>, Status> {
        let request = enforce_jwt(request)?;
        let renew = request.extensions().get::<RenewToken>().cloned();
        let mut resp = crate::logic::create_workspace::upsert_workspace(request).await?;
        apply_renew_metadata(renew, &mut resp);
        Ok(resp)
    }

    async fn list_workspaces(
        &self,
        request: Request<ListWorkspaceReq>,
    ) -> Result<Response<ListWorkspaceRsp>, Status> {
        let request = enforce_jwt(request)?;
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
        .add_service(HiveServiceServer::new(service))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let greeter = CrvHiveService::default();

    Server::builder()
        .add_service(HiveServiceServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
