//! 服务启动引导
use super::config::{BootstrapConfig, RuntimeConfig, RuntimeConfigSource};
use super::middleware::CombinedInterceptor;
use super::service::*;
use crate::daemon_server::db::DbManager;
use crate::daemon_server::state::AppState;
use crate::hive_client::HiveClient;
use crate::pb::changelist_service_server::ChangelistServiceServer;
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
    let mut app_state = AppState::new(db_arc.clone());

    // 尝试连接 HiveClient
    app_state = initialize_hive_client(app_state, &db_arc).await;

    let interceptor = CombinedInterceptor::new(app_state.clone());
    let system_service_impl = SystemServiceImpl::new(app_state.clone());
    let workspace_service_impl = WorkspaceServiceImpl::new(app_state.clone());
    let file_service_impl = FileServiceImpl::new(app_state.clone());
    let changelist_service_impl = ChangelistServiceImpl::new(app_state);

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
    let mut app_state = AppState::new(db_arc.clone());

    // 尝试连接 HiveClient
    app_state = initialize_hive_client(app_state, &db_arc).await;

    let interceptor = CombinedInterceptor::new(app_state.clone());
    let system_service_impl = SystemServiceImpl::new(app_state.clone());
    let workspace_service_impl = WorkspaceServiceImpl::new(app_state.clone());
    let file_service_impl = FileServiceImpl::new(app_state.clone());
    let changelist_service_impl = ChangelistServiceImpl::new(app_state);

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
            interceptor,
        ))
        .serve(addr)
        .await?;

    Ok(())
}

/// 初始化 HiveClient 连接
/// 
/// 从数据库读取 RuntimeConfig，获取 remote_addr 并连接到 Hive 服务器。
/// 如果连接失败，会打印警告信息但不会阻止 daemon 启动。
async fn initialize_hive_client(app_state: AppState, db: &Arc<DbManager>) -> AppState {
    // 1. 读取 RuntimeConfig
    let mut runtime_config = RuntimeConfig::default();
    
    match db.load_runtime_config() {
        Ok(config_override) => {
            runtime_config.merge(config_override, RuntimeConfigSource::Set);
        }
        Err(e) => {
            eprintln!("Warning: Failed to load runtime config from DB: {}", e);
            eprintln!("Using default remote_addr: {}", runtime_config.remote_addr.value);
        }
    }

    // 2. 获取 remote_addr 并转换为完整的 URL
    let remote_addr = runtime_config.remote_addr.value;
    
    // 检查地址是否已经包含协议前缀
    let hive_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
        remote_addr
    } else {
        // 默认使用 http:// 前缀
        format!("http://{}", remote_addr)
    };

    println!("Attempting to connect to Hive server at: {}", hive_url);

    // 3. 从 ChannelPool 获取 channel 并创建 HiveClient
    match app_state.hive_channel.get_channel(&hive_url) {
        Ok(channel) => {
            println!("Successfully got channel from pool for Hive server at {}", hive_url);
            
            // 使用 channel 创建 HiveClient
            let client = HiveClient::from_channel(channel);
            
            // 设置 JWT 持久化路径
            client.set_default_token_persist_path();
            
            // 尝试从磁盘加载 JWT
            match client.load_token_from_disk(None::<String>).await {
                Ok(true) => println!("Loaded JWT token from disk"),
                Ok(false) => println!("No JWT token found on disk"),
                Err(e) => eprintln!("Warning: Failed to load JWT from disk: {}", e),
            }
            
            // 将 HiveClient 添加到 AppState
            app_state.with_hive_client(client)
        }
        Err(e) => {
            eprintln!("Warning: Failed to get channel for Hive server at {}: {}", hive_url, e);
            eprintln!("Daemon will start without Hive connection");
            eprintln!("You can configure the Hive address using runtime config");
            app_state
        }
    }
}
