//! 服务启动引导
use super::config::BootstrapConfig;
use super::middleware::{auth::AuthInterceptor, config::ConfigInterceptor};
use super::service::CrvEdgeDaemonServiceImpl;
use crate::daemon_server::db::DbManager;
use crate::daemon_server::state::AppState;
use crate::pb::edge_daemon_service_server::EdgeDaemonServiceServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;

/// 启动 gRPC 服务器（优雅关闭）
pub async fn start_server_with_shutdown<S>(shutdown: S) -> Result<(), Box<dyn std::error::Error>>
where
    S: Future<Output = ()> + Send + 'static,
{
    let bootstrap_config = BootstrapConfig::load()?;
    let db = DbManager::new(bootstrap_config.embedded_database_root)?;
    let app_state = AppState::new(Arc::new(db));
    let edge_daemon_service_impl = CrvEdgeDaemonServiceImpl::new(app_state);

    let addr: SocketAddr = format!("127.0.0.1:{}", bootstrap_config.daemon_port).parse()?;

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(edge_daemon_service_impl))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap_config = BootstrapConfig::load()?;
    let db = DbManager::new(bootstrap_config.embedded_database_root)?;
    let app_state = AppState::new(Arc::new(db));
    let edge_daemon_service_impl = CrvEdgeDaemonServiceImpl::new(app_state);

    let addr: SocketAddr = format!("127.0.0.1:{}", bootstrap_config.daemon_port).parse()?;

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(edge_daemon_service_impl))
        .serve(addr)
        .await?;

    Ok(())
}
