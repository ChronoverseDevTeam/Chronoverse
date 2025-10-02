use tonic::{transport::Server, Request, Response, Status};
use crate::pb::hive_service_server::{HiveService, HiveServiceServer};
use crate::pb::{CreateWorkspaceReq, GreetingReq, ListWorkspaceReq, ListWorkspaceRsp, NilRsp};

#[derive(Default)]
pub struct CrvHiveService;

#[tonic::async_trait]
impl HiveService for CrvHiveService {
    async fn greeting(
        &self,
        request: Request<GreetingReq>,
    ) -> Result<Response<NilRsp>, Status> {
        // 由于 crate::logic::create_workspace::greeting 是异步函数，这里需要 .await
        crate::logic::create_workspace::greeting(request).await
    }

    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceReq>,
    ) -> Result<Response<NilRsp>, Status> {
        let _req = request.into_inner();
        Ok(Response::new(NilRsp {}))
    }

    async fn list_workspaces(
        &self,
        request: Request<ListWorkspaceReq>,
    ) -> Result<Response<ListWorkspaceRsp>, Status> {
        crate::logic::list_workspaces::list_workspaces(request).await
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
    let greeter = CrvHiveService::default();

    Server::builder()
        .add_service(HiveServiceServer::new(greeter))
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
