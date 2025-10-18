use tonic::{transport::Server, Request, Response, Status};
use crate::pb::edge_daemon_service_server::{EdgeDaemonService, EdgeDaemonServiceServer};
use crate::pb::{
    // Basic operations
    BonjourReq, BonjourRsp,
    // Workspace management
    CreateWorkspaceReq, CreateWorkspaceRsp, DeleteWorkspaceReq, DeleteWorkspaceRsp,
    ListWorkspacesReq, ListWorkspacesRsp, DescribeWorkspaceReq, DescribeWorkspaceRsp,
    CurrentWorkspaceReq, CurrentWorkspaceRsp,
    // File operations
    AddReq, AddRsp, SyncReq, SyncRsp, LockReq, LockRsp, UnlockReq, UnlockRsp,
    RevertReq, RevertRsp, SubmitReq, SubmitRsp,
    // Changelist management
    CreateChangelistReq, CreateChangelistRsp, DeleteChangelistReq, DeleteChangelistRsp,
    ListChangelistsReq, ListChangelistsRsp, DescribeChangelistReq, DescribeChangelistRsp,
    ChangelistInfo,
    // Snapshot management
    CreateSnapshotReq, CreateSnapshotRsp, DeleteSnapshotReq, DeleteSnapshotRsp,
    ListSnapshotsReq, ListSnapshotsRsp, DescribeSnapshotReq, DescribeSnapshotRsp,
    RestoreSnapshotReq, RestoreSnapshotRsp, SnapshotInfo,
    // Merge and resolve
    MergeReq, MergeRsp, ResolveReq, ResolveRsp,
    // Describe files
    DescribeReq, DescribeRsp, FileInfo,
    // Branch management
    CreateBranchReq, CreateBranchRsp, DeleteBranchReq, DeleteBranchRsp,
    ListBranchesReq, ListBranchesRsp, SwitchBranchReq, SwitchBranchRsp, BranchInfo,
    // Legacy operations
    GetLatestReq, GetLatestRsp, CheckoutReq, CheckoutRsp, SummitReq, SummitRsp,
};
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
        // 模拟处理逻辑
        let response = GetLatestRsp {
            success: true,
            message: "模拟获取最新文件列表".to_string(),
            file_paths: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        };
        Ok(Response::new(response))
    }

    async fn checkout(
        &self,
        request: Request<CheckoutReq>,
    ) -> Result<Response<CheckoutRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = CheckoutRsp {
            success: true,
            message: format!("模拟检出文件: {}", req.relative_path),
            file_path: req.relative_path,
        };
        Ok(Response::new(response))
    }

    async fn summit(
        &self,
        request: Request<SummitReq>,
    ) -> Result<Response<SummitRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = SummitRsp {
            success: true,
            message: format!("模拟提交文件: {}", req.relative_path),
            file_path: req.relative_path,
        };
        Ok(Response::new(response))
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

    // Workspace management
    async fn delete_workspace(
        &self,
        request: Request<DeleteWorkspaceReq>,
    ) -> Result<Response<DeleteWorkspaceRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DeleteWorkspaceRsp {
            success: true,
            message: format!("模拟删除工作空间: {}", req.workspace_name),
        };
        Ok(Response::new(response))
    }

    async fn list_workspaces(
        &self,
        _: Request<ListWorkspacesReq>,
    ) -> Result<Response<ListWorkspacesRsp>, Status> {
        // 模拟处理逻辑
        let response = ListWorkspacesRsp {
            success: true,
            message: "模拟获取工作空间列表".to_string(),
            workspace_names: vec!["workspace1".to_string(), "workspace2".to_string()],
        };
        Ok(Response::new(response))
    }

    async fn describe_workspace(
        &self,
        request: Request<DescribeWorkspaceReq>,
    ) -> Result<Response<DescribeWorkspaceRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DescribeWorkspaceRsp {
            success: true,
            message: format!("模拟描述工作空间: {}", req.workspace_name),
            workspace_name: req.workspace_name,
            workspace_path: "/mock/workspace/path".to_string(),
            file_paths: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        };
        Ok(Response::new(response))
    }

    async fn current_workspace(
        &self,
        _: Request<CurrentWorkspaceReq>,
    ) -> Result<Response<CurrentWorkspaceRsp>, Status> {
        // 模拟处理逻辑
        let response = CurrentWorkspaceRsp {
            success: true,
            message: "模拟获取当前工作空间".to_string(),
            workspace_name: "current_workspace".to_string(),
            workspace_path: "/mock/current/workspace".to_string(),
        };
        Ok(Response::new(response))
    }

    // File operations
    async fn add(
        &self,
        request: Request<AddReq>,
    ) -> Result<Response<AddRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = AddRsp {
            success: true,
            message: format!("模拟添加 {} 个文件", req.paths.len()),
            added_paths: req.paths,
        };
        Ok(Response::new(response))
    }

    async fn sync(
        &self,
        request: Request<SyncReq>,
    ) -> Result<Response<SyncRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = SyncRsp {
            success: true,
            message: format!("模拟同步 {} 个文件 (force: {})", req.depot_paths.len(), req.force),
            synced_paths: req.depot_paths,
        };
        Ok(Response::new(response))
    }

    async fn lock(
        &self,
        request: Request<LockReq>,
    ) -> Result<Response<LockRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = LockRsp {
            success: true,
            message: format!("模拟锁定 {} 个文件", req.paths.len()),
            locked_paths: req.paths,
        };
        Ok(Response::new(response))
    }

    async fn unlock(
        &self,
        request: Request<UnlockReq>,
    ) -> Result<Response<UnlockRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = UnlockRsp {
            success: true,
            message: format!("模拟解锁 {} 个文件", req.paths.len()),
            unlocked_paths: req.paths,
        };
        Ok(Response::new(response))
    }

    async fn revert(
        &self,
        request: Request<RevertReq>,
    ) -> Result<Response<RevertRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = RevertRsp {
            success: true,
            message: format!("模拟恢复 {} 个文件", req.paths.len()),
            reverted_paths: req.paths,
        };
        Ok(Response::new(response))
    }

    async fn submit(
        &self,
        request: Request<SubmitReq>,
    ) -> Result<Response<SubmitRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = SubmitRsp {
            success: true,
            message: format!("模拟提交变更列表 {}: {}", req.changelist_id, req.description),
            changelist_id: req.changelist_id,
        };
        Ok(Response::new(response))
    }

    // Changelist management
    async fn create_changelist(
        &self,
        request: Request<CreateChangelistReq>,
    ) -> Result<Response<CreateChangelistRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = CreateChangelistRsp {
            success: true,
            message: format!("模拟创建变更列表: {}", req.description),
            changelist_id: 12345,
        };
        Ok(Response::new(response))
    }

    async fn delete_changelist(
        &self,
        request: Request<DeleteChangelistReq>,
    ) -> Result<Response<DeleteChangelistRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DeleteChangelistRsp {
            success: true,
            message: format!("模拟删除变更列表: {}", req.changelist_id),
        };
        Ok(Response::new(response))
    }

    async fn list_changelists(
        &self,
        _: Request<ListChangelistsReq>,
    ) -> Result<Response<ListChangelistsRsp>, Status> {
        // 模拟处理逻辑
        let response = ListChangelistsRsp {
            success: true,
            message: "模拟获取变更列表".to_string(),
            changelists: vec![
                ChangelistInfo {
                    id: 1,
                    description: "模拟变更列表1".to_string(),
                    file_count: 5,
                    status: "pending".to_string(),
                },
                ChangelistInfo {
                    id: 2,
                    description: "模拟变更列表2".to_string(),
                    file_count: 3,
                    status: "submitted".to_string(),
                },
            ],
        };
        Ok(Response::new(response))
    }

    async fn describe_changelist(
        &self,
        request: Request<DescribeChangelistReq>,
    ) -> Result<Response<DescribeChangelistRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DescribeChangelistRsp {
            success: true,
            message: format!("模拟描述变更列表: {}", req.changelist_id),
            changelist: Some(ChangelistInfo {
                id: req.changelist_id,
                description: "模拟变更列表描述".to_string(),
                file_count: 3,
                status: "pending".to_string(),
            }),
            file_paths: if req.list_files {
                vec!["file1.txt".to_string(), "file2.txt".to_string(), "file3.txt".to_string()]
            } else {
                vec![]
            },
        };
        Ok(Response::new(response))
    }

    // Snapshot management
    async fn create_snapshot(
        &self,
        request: Request<CreateSnapshotReq>,
    ) -> Result<Response<CreateSnapshotRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = CreateSnapshotRsp {
            success: true,
            message: format!("模拟创建快照: {}", req.description),
            snapshot_id: format!("snapshot_{}", req.changelist_id),
        };
        Ok(Response::new(response))
    }

    async fn delete_snapshot(
        &self,
        request: Request<DeleteSnapshotReq>,
    ) -> Result<Response<DeleteSnapshotRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DeleteSnapshotRsp {
            success: true,
            message: format!("模拟删除快照: {}", req.snapshot_id),
        };
        Ok(Response::new(response))
    }

    async fn list_snapshots(
        &self,
        _: Request<ListSnapshotsReq>,
    ) -> Result<Response<ListSnapshotsRsp>, Status> {
        // 模拟处理逻辑
        let response = ListSnapshotsRsp {
            success: true,
            message: "模拟获取快照列表".to_string(),
            snapshots: vec![
                SnapshotInfo {
                    id: "snapshot_1".to_string(),
                    description: "模拟快照1".to_string(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                    file_count: 5,
                },
                SnapshotInfo {
                    id: "snapshot_2".to_string(),
                    description: "模拟快照2".to_string(),
                    created_at: "2024-01-02T00:00:00Z".to_string(),
                    file_count: 3,
                },
            ],
        };
        Ok(Response::new(response))
    }

    async fn describe_snapshot(
        &self,
        request: Request<DescribeSnapshotReq>,
    ) -> Result<Response<DescribeSnapshotRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DescribeSnapshotRsp {
            success: true,
            message: format!("模拟描述快照: {}", req.snapshot_id),
            snapshot: Some(SnapshotInfo {
                id: req.snapshot_id.clone(),
                description: "模拟快照描述".to_string(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
                file_count: 3,
            }),
            file_paths: vec!["file1.txt".to_string(), "file2.txt".to_string(), "file3.txt".to_string()],
        };
        Ok(Response::new(response))
    }

    async fn restore_snapshot(
        &self,
        request: Request<RestoreSnapshotReq>,
    ) -> Result<Response<RestoreSnapshotRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = RestoreSnapshotRsp {
            success: true,
            message: format!("模拟恢复快照: {}", req.snapshot_id),
            restored_paths: vec!["file1.txt".to_string(), "file2.txt".to_string()],
        };
        Ok(Response::new(response))
    }

    // Merge and resolve
    async fn merge(
        &self,
        request: Request<MergeReq>,
    ) -> Result<Response<MergeRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = MergeRsp {
            success: true,
            message: format!("模拟合并分支: {} ({} 个文件)", req.branch_name, req.depot_paths.len()),
            merged_paths: req.depot_paths.clone(),
            conflict_paths: vec!["conflict_file.txt".to_string()],
        };
        Ok(Response::new(response))
    }

    async fn resolve(
        &self,
        request: Request<ResolveReq>,
    ) -> Result<Response<ResolveRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = ResolveRsp {
            success: true,
            message: format!("模拟解决冲突: {} 个文件", req.paths.len()),
            resolved_paths: req.paths,
        };
        Ok(Response::new(response))
    }

    // Describe files
    async fn describe(
        &self,
        request: Request<DescribeReq>,
    ) -> Result<Response<DescribeRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DescribeRsp {
            success: true,
            message: format!("模拟描述文件: {} 个文件", req.paths.len()),
            files: req.paths.into_iter().map(|path| FileInfo {
                path: path.clone(),
                status: "modified".to_string(),
                last_modified: "2024-01-01T00:00:00Z".to_string(),
                size: 1024,
                hash: "mock_hash".to_string(),
            }).collect(),
        };
        Ok(Response::new(response))
    }

    // Branch management
    async fn create_branch(
        &self,
        request: Request<CreateBranchReq>,
    ) -> Result<Response<CreateBranchRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = CreateBranchRsp {
            success: true,
            message: format!("模拟创建分支: {} (基于 {})", req.branch_name, req.base_branch),
            branch_name: req.branch_name,
        };
        Ok(Response::new(response))
    }

    async fn delete_branch(
        &self,
        request: Request<DeleteBranchReq>,
    ) -> Result<Response<DeleteBranchRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = DeleteBranchRsp {
            success: true,
            message: format!("模拟删除分支: {}", req.branch_name),
        };
        Ok(Response::new(response))
    }

    async fn list_branches(
        &self,
        _: Request<ListBranchesReq>,
    ) -> Result<Response<ListBranchesRsp>, Status> {
        // 模拟处理逻辑
        let response = ListBranchesRsp {
            success: true,
            message: "模拟获取分支列表".to_string(),
            branches: vec![
                BranchInfo {
                    name: "main".to_string(),
                    is_current: true,
                    last_commit: "abc123".to_string(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                },
                BranchInfo {
                    name: "feature".to_string(),
                    is_current: false,
                    last_commit: "def456".to_string(),
                    created_at: "2024-01-02T00:00:00Z".to_string(),
                },
            ],
        };
        Ok(Response::new(response))
    }

    async fn switch_branch(
        &self,
        request: Request<SwitchBranchReq>,
    ) -> Result<Response<SwitchBranchRsp>, Status> {
        let req = request.into_inner();
        
        // 模拟处理逻辑
        let response = SwitchBranchRsp {
            success: true,
            message: format!("模拟切换分支: {}", req.branch_name),
            branch_name: req.branch_name,
        };
        Ok(Response::new(response))
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
