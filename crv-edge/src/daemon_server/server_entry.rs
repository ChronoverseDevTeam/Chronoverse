use tonic::{transport::Server, Request, Response, Status};
use crate::pb::edge_daemon_service_server::{EdgeDaemonService, EdgeDaemonServiceServer};
use crate::pb::{BonjourReq, BonjourRsp, GetLatestReq, GetLatestRsp, CheckoutReq, CheckoutRsp, SummitReq, SummitRsp, CreateWorkspaceReq, CreateWorkspaceRsp};
use crate::client_manager::workspace::WorkSpaceMetadata;
use std::future::Future;
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct CrvEdgeDaemonServer {
    workspace: Arc<Mutex<Option<Arc<Mutex<WorkSpaceMetadata>>>>>,
}

impl CrvEdgeDaemonServer {
    pub fn new() -> Self {
        Self {
            workspace: Arc::new(Mutex::new(None)),
        }
    }

    /// 创建工作空间
    fn create_workspace(&self) -> std::io::Result<PathBuf> {
        let mut workspace_opt = self.workspace.lock().unwrap();
        
        // 检查工作空间是否已存在
        if workspace_opt.is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "工作空间已存在"
            ));
        }

        // 使用默认路径创建工作空间
        let workspace_root = PathBuf::from("workspace");
        let depot_root = PathBuf::from("depot");
        
        std::fs::create_dir_all(&workspace_root)?;
        std::fs::create_dir_all(&depot_root)?;

        // 创建路径映射
        let mut path_mapping = HashMap::new();
        path_mapping.insert(workspace_root.clone(), depot_root.clone());

        // 创建工作空间管理器
        let workspace_manager = WorkSpaceMetadata::new(&workspace_root, &depot_root, path_mapping);
        
        // 存储工作空间（包装在 Arc<Mutex<>> 中）
        *workspace_opt = Some(Arc::new(Mutex::new(workspace_manager)));
        
        Ok(workspace_root)
    }
}

impl Default for CrvEdgeDaemonServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl EdgeDaemonService for CrvEdgeDaemonServer {
    async fn bonjour(
        &self,
        _: Request<BonjourReq>,
    ) -> Result<Response<BonjourRsp>, Status> {
        let response = BonjourRsp {
            daemon_version: "1.0.0".to_string(),
            api_level: 1,
            platform: "chronoverse".to_string(),
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
        };
        
        Ok(Response::new(response))
    }

    async fn get_latest(
        &self,
        _: Request<GetLatestReq>,
    ) -> Result<Response<GetLatestRsp>, Status> {
        // 使用本地工作空间管理器获取最新文件
        let workspace_opt = self.workspace.lock().unwrap();
        
        if let Some(workspace_arc) = workspace_opt.as_ref() {
            let mut workspace = workspace_arc.lock().unwrap();
            match workspace.get_latest() {
                Ok(_) => {
                    // 获取工作空间中的所有文件路径
                    let file_paths = workspace.get_file_paths();
                    
                    let response = GetLatestRsp {
                        success: true,
                        message: "获取最新文件列表成功".to_string(),
                        file_paths,
                    };
                    
                    Ok(Response::new(response))
                }
                Err(e) => {
                    let response = GetLatestRsp {
                        success: false,
                        message: format!("获取最新文件列表失败: {}", e),
                        file_paths: vec![],
                    };
                    
                    Ok(Response::new(response))
                }
            }
        } else {
            let response = GetLatestRsp {
                success: false,
                message: "没有可用的工作空间，请先创建工作空间".to_string(),
                file_paths: vec![],
            };
            
            Ok(Response::new(response))
        }
    }

    async fn checkout(
        &self,
        request: Request<CheckoutReq>,
    ) -> Result<Response<CheckoutRsp>, Status> {
        let req = request.into_inner();
        let relative_path = &req.relative_path;
        
        // 使用本地工作空间管理器检出文件
        let workspace_opt = self.workspace.lock().unwrap();
        
        if let Some(workspace_arc) = workspace_opt.as_ref() {
            let mut workspace = workspace_arc.lock().unwrap();
            let changelist_id = workspace.get_next_changelist_id();
            
            match workspace.checkout(relative_path.clone(), changelist_id) {
                Ok(_) => {
                    let response = CheckoutRsp {
                        success: true,
                        message: format!("成功检出文件: {} (changelist: {})", relative_path, changelist_id),
                        file_path: relative_path.clone(),
                    };
                    
                    Ok(Response::new(response))
                }
                Err(e) => {
                    let response = CheckoutRsp {
                        success: false,
                        message: format!("检出文件失败: {}", e),
                        file_path: relative_path.clone(),
                    };
                    
                    Ok(Response::new(response))
                }
            }
        } else {
            let response = CheckoutRsp {
                success: false,
                message: "没有可用的工作空间，请先创建工作空间".to_string(),
                file_path: relative_path.clone(),
            };
            
            Ok(Response::new(response))
        }
    }

    async fn summit(
        &self,
        request: Request<SummitReq>,
    ) -> Result<Response<SummitRsp>, Status> {
        let req = request.into_inner();
        let relative_path = &req.relative_path;
        
        // 使用本地工作空间管理器提交文件
        let workspace_opt = self.workspace.lock().unwrap();
        
        if let Some(workspace_arc) = workspace_opt.as_ref() {
            let mut workspace = workspace_arc.lock().unwrap();
            let changelist_id = workspace.get_next_changelist_id();
            let description = format!("提交文件: {}", relative_path);
            
            match workspace.submit_changelist(changelist_id, description) {
                Ok(_) => {
                    let response = SummitRsp {
                        success: true,
                        message: format!("成功提交文件: {} (changelist: {})", relative_path, changelist_id),
                        file_path: relative_path.clone(),
                    };
                    
                    Ok(Response::new(response))
                }
                Err(e) => {
                    let response = SummitRsp {
                        success: false,
                        message: format!("提交文件失败: {}", e),
                        file_path: relative_path.clone(),
                    };
                    
                    Ok(Response::new(response))
                }
            }
        } else {
            let response = SummitRsp {
                success: false,
                message: "没有可用的工作空间，请先创建工作空间".to_string(),
                file_path: relative_path.clone(),
            };
            
            Ok(Response::new(response))
        }
    }

    async fn create_workspace(
        &self,
        _: Request<CreateWorkspaceReq>,
    ) -> Result<Response<CreateWorkspaceRsp>, Status> {
        match self.create_workspace() {
            Ok(workspace_path) => {
                let response = CreateWorkspaceRsp {
                    success: true,
                    message: format!("成功创建工作空间在路径: {}", workspace_path.display()),
                    workspace_path: workspace_path.to_string_lossy().to_string(),
                };
                
                Ok(Response::new(response))
            }
            Err(e) => {
                let response = CreateWorkspaceRsp {
                    success: false,
                    message: format!("创建工作空间失败: {}", e),
                    workspace_path: "".to_string(),
                };
                
                Ok(Response::new(response))
            }
        }
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
    let daemon_server = CrvEdgeDaemonServer::default();

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(daemon_server))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let daemon_server = CrvEdgeDaemonServer::default();

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(daemon_server))
        .serve(addr)
        .await?;

    Ok(())
}