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
use crv_core::logger::{LogFormat, LogLevel, LogOutput, LogRotation, Logger};
use crv_core::{log_info, log_warn};
use directories::ProjectDirs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tonic::transport::Server;

/// 初始化全局 Logger（stdout + 按大小轮转的日志文件）。
/// 返回 LoggerGuard，调用方必须持有它，否则文件 appender 会被提前关闭。
fn init_logger() -> crv_core::logger::LoggerGuard {
    let log_path = ProjectDirs::from("com", "chronoverse", "crv-edge")
        .map(|dirs| dirs.data_local_dir().join("logs").join("edge.log"))
        .unwrap_or_else(|| PathBuf::from("logs/edge.log"));

    let rotation = LogRotation::SizeBased {
        max_size_mb: 50,
        compress: false,
    };

    let guard = Logger::builder()
        .level(LogLevel::Info)
        .format(LogFormat::Compact)
        .both(log_path.clone(), rotation)
        .build()
        .init();

    match guard {
        Ok(g) => {
            log_info!(log_path = %log_path.display(), "Logger initialized");
            g
        }
        Err(crv_core::logger::LogError::AlreadyInitialized) => {
            // 在测试或重入场景中可能发生，忽略即可
            log_warn!("Logger already initialized, skipping re-init");
            // 返回一个空 guard（Logger 已初始化，不需要新 guard）
            Logger::builder()
                .level(LogLevel::Info)
                .format(LogFormat::Compact)
                .stdout()
                .build()
                .init()
                .unwrap_or_else(|_| Logger::builder().stdout().build().init().unwrap())
        }
        Err(e) => {
            eprintln!("[crv-edge] Failed to initialize logger: {e}");
            // fallback: stdout only
            Logger::builder()
                .level(LogLevel::Info)
                .format(LogFormat::Compact)
                .stdout()
                .build()
                .init()
                .expect("Failed to initialize fallback stdout logger")
        }
    }
}

/// 启动 gRPC 服务器（优雅关闭）
pub async fn start_server_with_shutdown<S>(shutdown: S) -> Result<(), Box<dyn std::error::Error>>
where
    S: Future<Output = ()> + Send + 'static,
{
    let _logger_guard = init_logger();

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

    log_info!(addr = %addr, "Starting gRPC server");

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

    log_info!("gRPC server shut down gracefully");
    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let _logger_guard = init_logger();

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

    log_info!(addr = %addr, "Starting gRPC server");

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
