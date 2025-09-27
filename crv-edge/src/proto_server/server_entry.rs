use tonic::{transport::Server, Request, Response, Status};
use crate::pb::edge_daemon_service_server::{EdgeDaemonService, EdgeDaemonServiceServer};
use crate::pb::{GreetingReq, NilRsp};
use std::future::Future;

#[derive(Default)]
pub struct CvEdgeGreeter;

#[tonic::async_trait]
impl EdgeDaemonService for CvEdgeGreeter {
    async fn greeting(
        &self,
        request: Request<GreetingReq>,
    ) -> Result<Response<NilRsp>, Status> {
        let _msg = request.into_inner().msg;
        Ok(Response::new(NilRsp {}))
    }
}

/// 启动 gRPC 服务器（优雅关闭）
pub async fn start_server_with_shutdown<S>(
    addr: std::net::SocketAddr,
    shutdown: S,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: Future<Output = ()> + Send + 'static,
{
    let greeter = CvEdgeGreeter::default();

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(greeter))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let greeter = CvEdgeGreeter::default();

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}