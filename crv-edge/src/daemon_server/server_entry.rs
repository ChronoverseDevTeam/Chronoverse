use tonic::{transport::Server, Request, Response, Status};
use crate::pb::edge_daemon_service_server::{EdgeDaemonService, EdgeDaemonServiceServer};
use crate::pb::*;  // 导入所有 proto 生成的类型
use crate::client_manager::workspace::WorkSpaceMetadata;
use crate::hive_client::HiveClient;
use std::future::Future;
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CrvEdgeDaemonServerLocalTest {
    workspace: Arc<Mutex<Option<Arc<Mutex<WorkSpaceMetadata>>>>>,
    hive_client: Arc<TokioMutex<Option<HiveClient>>>,
    hive_endpoint: String,
}

impl CrvEdgeDaemonServerLocalTest {
    pub fn new() -> Self {
        Self {
            workspace: Arc::new(Mutex::new(None)),
            hive_client: Arc::new(TokioMutex::new(None)),
            hive_endpoint: "http://127.0.0.1:34560".to_string(),
        }
    }

    /// 确保 Hive 客户端已初始化（仅用于 Hive 相关方法）
    async fn ensure_hive_client(&self) -> Result<(), Status> {
        let mut client_guard = self.hive_client.lock().await;
        
        if client_guard.is_some() {
            return Ok(());
        }

        // 连接到 Hive
        let client = HiveClient::connect(&self.hive_endpoint)
            .await
            .map_err(|e| Status::unavailable(format!("Failed to connect to Hive: {}", e)))?;

        // 启用令牌持久化
        client.set_default_token_persist_path();
        
        // 尝试加载已保存的令牌
        let _ = client.load_token_from_disk(None::<std::path::PathBuf>).await;

        *client_guard = Some(client);
        Ok(())
    }

    /// 获取 Hive 客户端的克隆（仅用于 Hive 相关方法）
    async fn get_hive_client(&self) -> Result<HiveClient, Status> {
        let client_guard = self.hive_client.lock().await;
        client_guard.clone()
            .ok_or_else(|| Status::unavailable("Hive client not initialized"))
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

    /// 获取当前时间戳
    fn get_current_timestamp(&self) -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    }

    /// 模拟本地文件缓存操作
    fn simulate_local_file_operation(&self, operation: &str, file_path: &str) -> Result<String, String> {
        // 模拟文件操作延迟
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        // 模拟操作结果
        match operation {
            "checkout" => {
                // 模拟检出文件到工作区
                let checkout_entry = format!("checked_out_{}_{}", self.get_current_timestamp(), file_path);
                Ok(format!("文件 {} 已检出到工作区: {}", file_path, checkout_entry))
            }
            "submit" => {
                // 模拟提交文件到服务器
                let submit_entry = format!("submitted_{}_{}", self.get_current_timestamp(), file_path);
                Ok(format!("文件 {} 已提交到服务器: {}", file_path, submit_entry))
            }
            "get_latest" => {
                // 模拟获取最新文件
                let latest_entry = format!("latest_{}_{}", self.get_current_timestamp(), file_path);
                Ok(format!("文件 {} 最新版本: {}", file_path, latest_entry))
            }
            _ => Err(format!("未知操作: {}", operation))
        }
    }
}

impl Default for CrvEdgeDaemonServerLocalTest {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl EdgeDaemonService for CrvEdgeDaemonServerLocalTest {
    // 基本操作
    async fn bonjour(&self, _: Request<BonjourReq>) -> Result<Response<BonjourRsp>, Status> {
        let response = BonjourRsp {
            daemon_version: "1.0.0-local-test".to_string(),
            api_level: 1,
            platform: "chronoverse".to_string(),
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
        };
        Ok(Response::new(response))
    }

    // 工作空间管理 - 保留基本实现
    async fn create_workspace(&self, _: Request<CreateWorkspaceReq>) -> Result<Response<CreateWorkspaceRsp>, Status> {
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

    // 关键指令 1: checkout
    async fn checkout(&self, request: Request<CheckoutReq>) -> Result<Response<CheckoutRsp>, Status> {
        let req = request.into_inner();
        let workspace_opt = self.workspace.lock().unwrap();
        
        if let Some(workspace_arc) = workspace_opt.as_ref() {
            let mut workspace = workspace_arc.lock().unwrap();
            let changelist_id = workspace.get_next_changelist_id();
            
            match self.simulate_local_file_operation("checkout", &req.relative_path) {
                Ok(sim_result) => {
                    match workspace.checkout(req.relative_path.clone(), changelist_id) {
                        Ok(_) => {
                            let response = CheckoutRsp {
                                success: true,
                                message: format!("成功检出文件: {} (changelist: {}) - {}", 
                                               req.relative_path, changelist_id, sim_result),
                                file_path: req.relative_path,
                            };
                            Ok(Response::new(response))
                        }
                        Err(e) => {
                            let response = CheckoutRsp {
                                success: false,
                                message: format!("检出文件失败: {} - {}", e, sim_result),
                                file_path: req.relative_path,
                            };
                            Ok(Response::new(response))
                        }
                    }
                }
                Err(e) => {
                    let response = CheckoutRsp {
                        success: false,
                        message: format!("检出文件失败: {}", e),
                        file_path: req.relative_path,
                    };
                    Ok(Response::new(response))
                }
            }
        } else {
            let response = CheckoutRsp {
                success: false,
                message: "没有可用的工作空间，请先创建工作空间".to_string(),
                file_path: req.relative_path,
            };
            Ok(Response::new(response))
        }
    }

    // 关键指令 2: submit
    async fn submit(&self, request: Request<SubmitReq>) -> Result<Response<SubmitRsp>, Status> {
        let req = request.into_inner();
        let workspace_opt = self.workspace.lock().unwrap();
        
        if let Some(workspace_arc) = workspace_opt.as_ref() {
            let mut workspace = workspace_arc.lock().unwrap();
            
            let submit_desc = format!("{} - 提交时间: {}", req.description, self.get_current_timestamp());
            match self.simulate_local_file_operation("submit", &format!("changelist_{}", req.changelist_id)) {
                Ok(sim_result) => {
                    match workspace.submit_changelist(req.changelist_id as u32, submit_desc) {
                        Ok(_) => {
                            let response = SubmitRsp {
                                success: true,
                                message: format!("成功提交变更列表 {}: {} - {}", 
                                               req.changelist_id, req.description, sim_result),
                                changelist_id: req.changelist_id,
                            };
                            Ok(Response::new(response))
                        }
                        Err(e) => {
                            let response = SubmitRsp {
                                success: false,
                                message: format!("提交失败: {} - {}", e, sim_result),
                                changelist_id: req.changelist_id,
                            };
                            Ok(Response::new(response))
                        }
                    }
                }
                Err(e) => {
                    let response = SubmitRsp {
                        success: false,
                        message: format!("提交失败: {}", e),
                        changelist_id: req.changelist_id,
                    };
                    Ok(Response::new(response))
                }
            }
        } else {
            let response = SubmitRsp {
                success: false,
                message: "没有可用的工作空间，请先创建工作空间".to_string(),
                changelist_id: req.changelist_id,
            };
            Ok(Response::new(response))
        }
    }

    // 关键指令 3: get_latest
    async fn get_latest(&self, _: Request<GetLatestReq>) -> Result<Response<GetLatestRsp>, Status> {
        let workspace_opt = self.workspace.lock().unwrap();
        
        if let Some(workspace_arc) = workspace_opt.as_ref() {
            let mut workspace = workspace_arc.lock().unwrap();
            
            match workspace.get_latest() {
                Ok(_) => {
                    let file_paths = workspace.get_file_paths();
                    let mut latest_files = Vec::new();
                    for file_path in file_paths {
                        match self.simulate_local_file_operation("get_latest", &file_path) {
                            Ok(sim_result) => {
                                latest_files.push(format!("{} - {}", file_path, sim_result));
                            }
                            Err(e) => {
                                latest_files.push(format!("{} - 错误: {}", file_path, e));
                            }
                        }
                    }
                    
                    let response = GetLatestRsp {
                        success: true,
                        message: format!("成功获取最新文件列表，共 {} 个文件", latest_files.len()),
                        file_paths: latest_files,
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

    // 关键指令 4: summit (兼容性方法)
    async fn summit(&self, request: Request<SummitReq>) -> Result<Response<SummitRsp>, Status> {
        let req = request.into_inner();
        
        let submit_req = SubmitReq {
            changelist_id: 1,
            description: format!("Summit: {}", req.relative_path),
        };
        
        match self.submit(Request::new(submit_req)).await {
            Ok(submit_response) => {
                let submit_rsp = submit_response.into_inner();
                let response = SummitRsp {
                    success: submit_rsp.success,
                    message: submit_rsp.message.replace("提交变更列表", "Summit"),
                    file_path: req.relative_path,
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                let response = SummitRsp {
                    success: false,
                    message: format!("Summit失败: {}", e),
                    file_path: req.relative_path,
                };
                Ok(Response::new(response))
            }
        }
    }

    // 以下是其他必需方法的空实现
    async fn delete_workspace(&self, _: Request<DeleteWorkspaceReq>) -> Result<Response<DeleteWorkspaceRsp>, Status> {
        Ok(Response::new(DeleteWorkspaceRsp {
            success: false,
            message: "未实现的方法".to_string(),
        }))
    }

    async fn list_workspaces(&self, _: Request<ListWorkspacesReq>) -> Result<Response<ListWorkspacesRsp>, Status> {
        Ok(Response::new(ListWorkspacesRsp {
            success: false,
            message: "未实现的方法".to_string(),
            workspace_names: vec![],
        }))
    }

    async fn describe_workspace(&self, _: Request<DescribeWorkspaceReq>) -> Result<Response<DescribeWorkspaceRsp>, Status> {
        Ok(Response::new(DescribeWorkspaceRsp {
            success: false,
            message: "未实现的方法".to_string(),
            workspace_name: String::new(),
            workspace_path: String::new(),
            file_paths: vec![],
        }))
    }

    async fn current_workspace(&self, _: Request<CurrentWorkspaceReq>) -> Result<Response<CurrentWorkspaceRsp>, Status> {
        Ok(Response::new(CurrentWorkspaceRsp {
            success: false,
            message: "未实现的方法".to_string(),
            workspace_name: String::new(),
            workspace_path: String::new(),
        }))
    }

    async fn add(&self, _: Request<AddReq>) -> Result<Response<AddRsp>, Status> {
        Ok(Response::new(AddRsp {
            success: false,
            message: "未实现的方法".to_string(),
            added_paths: vec![],
        }))
    }

    async fn sync(&self, _: Request<SyncReq>) -> Result<Response<SyncRsp>, Status> {
        Ok(Response::new(SyncRsp {
            success: false,
            message: "未实现的方法".to_string(),
            synced_paths: vec![],
        }))
    }

    async fn lock(&self, _: Request<LockReq>) -> Result<Response<LockRsp>, Status> {
        Ok(Response::new(LockRsp {
            success: false,
            message: "未实现的方法".to_string(),
            locked_paths: vec![],
        }))
    }

    async fn unlock(&self, _: Request<UnlockReq>) -> Result<Response<UnlockRsp>, Status> {
        Ok(Response::new(UnlockRsp {
            success: false,
            message: "未实现的方法".to_string(),
            unlocked_paths: vec![],
        }))
    }

    async fn revert(&self, _: Request<RevertReq>) -> Result<Response<RevertRsp>, Status> {
        Ok(Response::new(RevertRsp {
            success: false,
            message: "未实现的方法".to_string(),
            reverted_paths: vec![],
        }))
    }

    async fn create_changelist(&self, _: Request<CreateChangelistReq>) -> Result<Response<CreateChangelistRsp>, Status> {
        Ok(Response::new(CreateChangelistRsp {
            success: false,
            message: "未实现的方法".to_string(),
            changelist_id: 0,
        }))
    }

    async fn delete_changelist(&self, _: Request<DeleteChangelistReq>) -> Result<Response<DeleteChangelistRsp>, Status> {
        Ok(Response::new(DeleteChangelistRsp {
            success: false,
            message: "未实现的方法".to_string(),
        }))
    }

    async fn list_changelists(&self, _: Request<ListChangelistsReq>) -> Result<Response<ListChangelistsRsp>, Status> {
        Ok(Response::new(ListChangelistsRsp {
            success: false,
            message: "未实现的方法".to_string(),
            changelists: vec![],
        }))
    }

    async fn describe_changelist(&self, _: Request<DescribeChangelistReq>) -> Result<Response<DescribeChangelistRsp>, Status> {
        Ok(Response::new(DescribeChangelistRsp {
            success: false,
            message: "未实现的方法".to_string(),
            changelist: None,
            file_paths: vec![],
        }))
    }

    async fn create_snapshot(&self, _: Request<CreateSnapshotReq>) -> Result<Response<CreateSnapshotRsp>, Status> {
        Ok(Response::new(CreateSnapshotRsp {
            success: false,
            message: "未实现的方法".to_string(),
            snapshot_id: String::new(),
        }))
    }

    async fn delete_snapshot(&self, _: Request<DeleteSnapshotReq>) -> Result<Response<DeleteSnapshotRsp>, Status> {
        Ok(Response::new(DeleteSnapshotRsp {
            success: false,
            message: "未实现的方法".to_string(),
        }))
    }

    async fn list_snapshots(&self, _: Request<ListSnapshotsReq>) -> Result<Response<ListSnapshotsRsp>, Status> {
        Ok(Response::new(ListSnapshotsRsp {
            success: false,
            message: "未实现的方法".to_string(),
            snapshots: vec![],
        }))
    }

    async fn describe_snapshot(&self, _: Request<DescribeSnapshotReq>) -> Result<Response<DescribeSnapshotRsp>, Status> {
        Ok(Response::new(DescribeSnapshotRsp {
            success: false,
            message: "未实现的方法".to_string(),
            snapshot: None,
            file_paths: vec![],
        }))
    }

    async fn restore_snapshot(&self, _: Request<RestoreSnapshotReq>) -> Result<Response<RestoreSnapshotRsp>, Status> {
        Ok(Response::new(RestoreSnapshotRsp {
            success: false,
            message: "未实现的方法".to_string(),
            restored_paths: vec![],
        }))
    }

    async fn merge(&self, _: Request<MergeReq>) -> Result<Response<MergeRsp>, Status> {
        Ok(Response::new(MergeRsp {
            success: false,
            message: "未实现的方法".to_string(),
            merged_paths: vec![],
            conflict_paths: vec![],
        }))
    }

    async fn resolve(&self, _: Request<ResolveReq>) -> Result<Response<ResolveRsp>, Status> {
        Ok(Response::new(ResolveRsp {
            success: false,
            message: "未实现的方法".to_string(),
            resolved_paths: vec![],
        }))
    }

    async fn describe(&self, _: Request<DescribeReq>) -> Result<Response<DescribeRsp>, Status> {
        Ok(Response::new(DescribeRsp {
            success: false,
            message: "未实现的方法".to_string(),
            files: vec![],
        }))
    }

    async fn create_branch(&self, _: Request<CreateBranchReq>) -> Result<Response<CreateBranchRsp>, Status> {
        Ok(Response::new(CreateBranchRsp {
            success: false,
            message: "未实现的方法".to_string(),
            branch_name: String::new(),
        }))
    }

    async fn delete_branch(&self, _: Request<DeleteBranchReq>) -> Result<Response<DeleteBranchRsp>, Status> {
        Ok(Response::new(DeleteBranchRsp {
            success: false,
            message: "未实现的方法".to_string(),
        }))
    }

    async fn list_branches(&self, _: Request<ListBranchesReq>) -> Result<Response<ListBranchesRsp>, Status> {
        Ok(Response::new(ListBranchesRsp {
            success: false,
            message: "未实现的方法".to_string(),
            branches: vec![],
        }))
    }

    async fn switch_branch(&self, _: Request<SwitchBranchReq>) -> Result<Response<SwitchBranchRsp>, Status> {
        Ok(Response::new(SwitchBranchRsp {
            success: false,
            message: "未实现的方法".to_string(),
            branch_name: String::new(),
        }))
    }

    // Hive 集成方法
    async fn hive_connect(&self, request: Request<HiveConnectReq>) -> Result<Response<HiveConnectRsp>, Status> {
        let req = request.into_inner();
        
        // 如果提供了自定义地址，更新 hive_endpoint（这里简化处理，实际可能需要更复杂的逻辑）
        // 注意：由于 hive_endpoint 是不可变的，这里只是尝试连接
        
        match self.ensure_hive_client().await {
            Ok(_) => {
                let response = HiveConnectRsp {
                    success: true,
                    message: format!("已连接到 Hive 服务器: {}", 
                        if req.hive_address.is_empty() { &self.hive_endpoint } else { &req.hive_address }),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                let response = HiveConnectRsp {
                    success: false,
                    message: format!("连接 Hive 失败: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }

    async fn hive_login(&self, request: Request<HiveLoginReq>) -> Result<Response<HiveLoginRsp>, Status> {
        let req = request.into_inner();
        
        // 确保 Hive 客户端已初始化
        self.ensure_hive_client().await?;
        
        // 获取 Hive 客户端
        let mut hive_client = self.get_hive_client().await?;
        
        // 调用 Hive 登录
        match hive_client.login(req.username.clone(), req.password).await {
            Ok(login_rsp) => {
                let response = HiveLoginRsp {
                    success: true,
                    message: format!("用户 '{}' 登录成功", req.username),
                    access_token: login_rsp.access_token.clone(),
                    expires_at: login_rsp.expires_at,
                };
                
                // 更新客户端中的令牌
                let mut client_guard = self.hive_client.lock().await;
                *client_guard = Some(hive_client);
                
                Ok(Response::new(response))
            }
            Err(e) => {
                let response = HiveLoginRsp {
                    success: false,
                    message: format!("登录失败: {}", e),
                    access_token: String::new(),
                    expires_at: 0,
                };
                Ok(Response::new(response))
            }
        }
    }

    async fn hive_register(&self, request: Request<HiveRegisterReq>) -> Result<Response<HiveRegisterRsp>, Status> {
        let req = request.into_inner();
        
        // 确保 Hive 客户端已初始化
        self.ensure_hive_client().await?;
        
        // 获取 Hive 客户端
        let mut hive_client = self.get_hive_client().await?;
        
        // 调用 Hive 注册
        match hive_client.register(req.username.clone(), req.password, req.email).await {
            Ok(_) => {
                let response = HiveRegisterRsp {
                    success: true,
                    message: format!("用户 '{}' 注册成功", req.username),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                let response = HiveRegisterRsp {
                    success: false,
                    message: format!("注册失败: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }

    async fn hive_list_workspaces(&self, request: Request<HiveListWorkspacesReq>) -> Result<Response<HiveListWorkspacesRsp>, Status> {
        let req = request.into_inner();
        
        // 确保 Hive 客户端已初始化
        self.ensure_hive_client().await?;
        
        // 获取 Hive 客户端
        let mut hive_client = self.get_hive_client().await?;
        
        // 调用 Hive 的 list_workspaces
        match hive_client.list_workspaces(req.name, req.owner, req.device_finger_print).await {
            Ok(hive_rsp) => {
                // 转换为 Edge 响应格式
                let workspaces: Vec<HiveWorkspaceInfo> = hive_rsp.workspaces
                    .into_iter()
                    .map(|ws| HiveWorkspaceInfo {
                        name: ws.name,
                        owner: ws.owner,
                        path: ws.path,
                    })
                    .collect();
                
                let response = HiveListWorkspacesRsp {
                    success: true,
                    message: format!("获取到 {} 个工作空间", workspaces.len()),
                    workspaces,
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                let response = HiveListWorkspacesRsp {
                    success: false,
                    message: format!("获取工作空间列表失败: {}", e),
                    workspaces: vec![],
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
    let daemon_server = CrvEdgeDaemonServerLocalTest::default();

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(daemon_server))
        .serve_with_shutdown(addr, shutdown)
        .await?;

    Ok(())
}

/// 启动 gRPC 服务器（无关闭信号，会一直运行直至进程退出）
pub async fn start_server(addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let daemon_server = CrvEdgeDaemonServerLocalTest::default();

    Server::builder()
        .add_service(EdgeDaemonServiceServer::new(daemon_server))
        .serve(addr)
        .await?;

    Ok(())
}

