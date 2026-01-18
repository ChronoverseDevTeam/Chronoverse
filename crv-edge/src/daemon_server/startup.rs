//! 服务启动引导
use super::config::BootstrapConfig;
use super::middleware::CombinedInterceptor;
use super::service::*;
use crate::daemon_server::db::DbManager;
use crate::daemon_server::state::AppState;
use crate::pb::changelist_service_server::ChangelistServiceServer;
use crate::pb::debug_service_server::DebugServiceServer;
use crate::pb::file_service_server::FileServiceServer;
use crate::pb::system_service_server::SystemServiceServer;
use crate::pb::workspace_service_server::WorkspaceServiceServer;
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
    let db_arc = Arc::new(db);
    let app_state = AppState::new(db_arc.clone());

    let interceptor = CombinedInterceptor::new(app_state.clone());
    let system_service_impl = SystemServiceImpl::new(app_state.clone());
    let workspace_service_impl = WorkspaceServiceImpl::new(app_state.clone());
    let file_service_impl = FileServiceImpl::new(app_state.clone());
    let changelist_service_impl = ChangelistServiceImpl::new(app_state.clone());
    let debug_service_impl = DebugServiceImpl::new(app_state);

    let addr: SocketAddr = format!("[::1]:{}", bootstrap_config.daemon_port).parse()?;

    println!("Starting gRPC server on {}", addr);

    Server::builder()
        .add_service(SystemServiceServer::with_interceptor(
            system_service_impl,
            interceptor.clone(),
        ))
        .add_service(WorkspaceServiceServer::with_interceptor(
            workspace_service_impl,
            interceptor.clone(),
        ))
        .add_service(FileServiceServer::with_interceptor(
            file_service_impl,
            interceptor.clone(),
        ))
        .add_service(ChangelistServiceServer::with_interceptor(
            changelist_service_impl,
            interceptor.clone(),
        ))
        .add_service(DebugServiceServer::with_interceptor(
            debug_service_impl,
            interceptor,
        ))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap_config = BootstrapConfig::load()?;
    let db = DbManager::new(bootstrap_config.embedded_database_root)?;
    let db_arc = Arc::new(db);
    let app_state = AppState::new(db_arc.clone());

    let interceptor = CombinedInterceptor::new(app_state.clone());
    let system_service_impl = SystemServiceImpl::new(app_state.clone());
    let workspace_service_impl = WorkspaceServiceImpl::new(app_state.clone());
    let file_service_impl = FileServiceImpl::new(app_state.clone());
    let changelist_service_impl = ChangelistServiceImpl::new(app_state.clone());
    let debug_service_impl = DebugServiceImpl::new(app_state);

    let addr: SocketAddr = format!("[::1]:{}", bootstrap_config.daemon_port).parse()?;

    Server::builder()
        .add_service(SystemServiceServer::with_interceptor(
            system_service_impl,
            interceptor.clone(),
        ))
        .add_service(WorkspaceServiceServer::with_interceptor(
            workspace_service_impl,
            interceptor.clone(),
        ))
        .add_service(FileServiceServer::with_interceptor(
            file_service_impl,
            interceptor.clone(),
        ))
        .add_service(ChangelistServiceServer::with_interceptor(
            changelist_service_impl,
            interceptor.clone(),
        ))
        .add_service(DebugServiceServer::with_interceptor(
            debug_service_impl,
            interceptor,
        ))
        .serve(addr)
        .await?;

    Ok(())
}
