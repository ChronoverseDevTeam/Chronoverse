use tonic::transport::Channel;
use crate::pb::edge_daemon_service_client::EdgeDaemonServiceClient;
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
    // Snapshot management
    CreateSnapshotReq, CreateSnapshotRsp, DeleteSnapshotReq, DeleteSnapshotRsp,
    ListSnapshotsReq, ListSnapshotsRsp, DescribeSnapshotReq, DescribeSnapshotRsp,
    RestoreSnapshotReq, RestoreSnapshotRsp,
    // Merge and resolve
    MergeReq, MergeRsp, ResolveReq, ResolveRsp,
    // Describe files
    DescribeReq, DescribeRsp,
    // Branch management
    CreateBranchReq, CreateBranchRsp, DeleteBranchReq, DeleteBranchRsp,
    ListBranchesReq, ListBranchesRsp, SwitchBranchReq, SwitchBranchRsp,
    // Legacy operations
    GetLatestReq, GetLatestRsp, CheckoutReq, CheckoutRsp, SummitReq, SummitRsp,
};

/// gRPC 客户端结构体
pub struct CrvClient {
    client: EdgeDaemonServiceClient<Channel>,
}

impl CrvClient {
    /// 创建新的客户端实例
    pub async fn new(server_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let channel = Channel::from_shared(server_addr.to_string())?
            .connect()
            .await?;
        
        let client = EdgeDaemonServiceClient::new(channel);
        
        Ok(Self { client })
    }

    /// 发送问候消息到服务器
    pub async fn bonjour(&mut self) -> Result<BonjourRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(BonjourReq {});

        let response: tonic::Response<BonjourRsp> = self.client.bonjour(request).await?;
        
        println!("服务器响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 获取最新版本的文件列表
    pub async fn get_latest(&mut self) -> Result<GetLatestRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(GetLatestReq {});

        let response: tonic::Response<GetLatestRsp> = self.client.get_latest(request).await?;
        
        println!("GetLatest 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 检出指定路径的文件
    pub async fn checkout(&mut self, relative_path: &str) -> Result<CheckoutRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(CheckoutReq {
            relative_path: relative_path.to_string(),
        });

        let response: tonic::Response<CheckoutRsp> = self.client.checkout(request).await?;
        
        println!("Checkout 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 提交指定路径的文件
    pub async fn summit(&mut self, relative_path: &str) -> Result<SummitRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(SummitReq {
            relative_path: relative_path.to_string(),
        });

        let response: tonic::Response<SummitRsp> = self.client.summit(request).await?;
        
        println!("Summit 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 创建工作空间
    pub async fn create_workspace(&mut self) -> Result<CreateWorkspaceRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(CreateWorkspaceReq {});

        let response: tonic::Response<CreateWorkspaceRsp> = self.client.create_workspace(request).await?;
        
        println!("CreateWorkspace 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // Workspace management
    /// 删除工作空间
    pub async fn delete_workspace(&mut self, workspace_name: String) -> Result<DeleteWorkspaceRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DeleteWorkspaceReq { workspace_name });
        let response: tonic::Response<DeleteWorkspaceRsp> = self.client.delete_workspace(request).await?;
        println!("DeleteWorkspace 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 列出所有工作空间
    pub async fn list_workspaces(&mut self) -> Result<ListWorkspacesRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(ListWorkspacesReq {});
        let response: tonic::Response<ListWorkspacesRsp> = self.client.list_workspaces(request).await?;
        println!("ListWorkspaces 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 描述工作空间
    pub async fn describe_workspace(&mut self, workspace_name: String) -> Result<DescribeWorkspaceRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DescribeWorkspaceReq { workspace_name });
        let response: tonic::Response<DescribeWorkspaceRsp> = self.client.describe_workspace(request).await?;
        println!("DescribeWorkspace 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 获取当前工作空间
    pub async fn current_workspace(&mut self) -> Result<CurrentWorkspaceRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(CurrentWorkspaceReq {});
        let response: tonic::Response<CurrentWorkspaceRsp> = self.client.current_workspace(request).await?;
        println!("CurrentWorkspace 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // File operations
    /// 添加文件到版本控制
    pub async fn add(&mut self, paths: Vec<String>) -> Result<AddRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(AddReq { paths });
        let response: tonic::Response<AddRsp> = self.client.add(request).await?;
        println!("Add 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 同步文件
    pub async fn sync(&mut self, depot_paths: Vec<String>, force: bool) -> Result<SyncRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(SyncReq { depot_paths, force });
        let response: tonic::Response<SyncRsp> = self.client.sync(request).await?;
        println!("Sync 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 锁定文件
    pub async fn lock(&mut self, paths: Vec<String>) -> Result<LockRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(LockReq { paths });
        let response: tonic::Response<LockRsp> = self.client.lock(request).await?;
        println!("Lock 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 解锁文件
    pub async fn unlock(&mut self, paths: Vec<String>) -> Result<UnlockRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(UnlockReq { paths });
        let response: tonic::Response<UnlockRsp> = self.client.unlock(request).await?;
        println!("Unlock 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 恢复文件
    pub async fn revert(&mut self, paths: Vec<String>) -> Result<RevertRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(RevertReq { paths });
        let response: tonic::Response<RevertRsp> = self.client.revert(request).await?;
        println!("Revert 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 提交文件
    pub async fn submit(&mut self, changelist_id: i32, description: String) -> Result<SubmitRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(SubmitReq { changelist_id, description });
        let response: tonic::Response<SubmitRsp> = self.client.submit(request).await?;
        println!("Submit 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // Changelist management
    /// 创建变更列表
    pub async fn create_changelist(&mut self, description: String) -> Result<CreateChangelistRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(CreateChangelistReq { description });
        let response: tonic::Response<CreateChangelistRsp> = self.client.create_changelist(request).await?;
        println!("CreateChangelist 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 删除变更列表
    pub async fn delete_changelist(&mut self, changelist_id: i32) -> Result<DeleteChangelistRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DeleteChangelistReq { changelist_id });
        let response: tonic::Response<DeleteChangelistRsp> = self.client.delete_changelist(request).await?;
        println!("DeleteChangelist 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 列出所有变更列表
    pub async fn list_changelists(&mut self) -> Result<ListChangelistsRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(ListChangelistsReq {});
        let response: tonic::Response<ListChangelistsRsp> = self.client.list_changelists(request).await?;
        println!("ListChangelists 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 描述变更列表
    pub async fn describe_changelist(&mut self, changelist_id: i32, list_files: bool) -> Result<DescribeChangelistRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DescribeChangelistReq { changelist_id, list_files });
        let response: tonic::Response<DescribeChangelistRsp> = self.client.describe_changelist(request).await?;
        println!("DescribeChangelist 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // Snapshot management
    /// 创建快照
    pub async fn create_snapshot(&mut self, changelist_id: i32, description: String) -> Result<CreateSnapshotRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(CreateSnapshotReq { changelist_id, description });
        let response: tonic::Response<CreateSnapshotRsp> = self.client.create_snapshot(request).await?;
        println!("CreateSnapshot 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 删除快照
    pub async fn delete_snapshot(&mut self, snapshot_id: String) -> Result<DeleteSnapshotRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DeleteSnapshotReq { snapshot_id });
        let response: tonic::Response<DeleteSnapshotRsp> = self.client.delete_snapshot(request).await?;
        println!("DeleteSnapshot 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 列出所有快照
    pub async fn list_snapshots(&mut self) -> Result<ListSnapshotsRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(ListSnapshotsReq {});
        let response: tonic::Response<ListSnapshotsRsp> = self.client.list_snapshots(request).await?;
        println!("ListSnapshots 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 描述快照
    pub async fn describe_snapshot(&mut self, snapshot_id: String) -> Result<DescribeSnapshotRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DescribeSnapshotReq { snapshot_id });
        let response: tonic::Response<DescribeSnapshotRsp> = self.client.describe_snapshot(request).await?;
        println!("DescribeSnapshot 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 恢复快照
    pub async fn restore_snapshot(&mut self, snapshot_id: String) -> Result<RestoreSnapshotRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(RestoreSnapshotReq { snapshot_id });
        let response: tonic::Response<RestoreSnapshotRsp> = self.client.restore_snapshot(request).await?;
        println!("RestoreSnapshot 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // Merge and resolve
    /// 合并分支
    pub async fn merge(&mut self, branch_name: String, depot_paths: Vec<String>) -> Result<MergeRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(MergeReq { branch_name, depot_paths });
        let response: tonic::Response<MergeRsp> = self.client.merge(request).await?;
        println!("Merge 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 解决冲突
    pub async fn resolve(&mut self, paths: Vec<String>) -> Result<ResolveRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(ResolveReq { paths });
        let response: tonic::Response<ResolveRsp> = self.client.resolve(request).await?;
        println!("Resolve 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // Describe files
    /// 描述文件状态
    pub async fn describe(&mut self, paths: Vec<String>) -> Result<DescribeRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DescribeReq { paths });
        let response: tonic::Response<DescribeRsp> = self.client.describe(request).await?;
        println!("Describe 响应: {:?}", response);
        Ok(response.into_inner())
    }

    // Branch management
    /// 创建分支
    pub async fn create_branch(&mut self, branch_name: String, base_branch: String) -> Result<CreateBranchRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(CreateBranchReq { branch_name, base_branch });
        let response: tonic::Response<CreateBranchRsp> = self.client.create_branch(request).await?;
        println!("CreateBranch 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 删除分支
    pub async fn delete_branch(&mut self, branch_name: String) -> Result<DeleteBranchRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(DeleteBranchReq { branch_name });
        let response: tonic::Response<DeleteBranchRsp> = self.client.delete_branch(request).await?;
        println!("DeleteBranch 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 列出所有分支
    pub async fn list_branches(&mut self) -> Result<ListBranchesRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(ListBranchesReq {});
        let response: tonic::Response<ListBranchesRsp> = self.client.list_branches(request).await?;
        println!("ListBranches 响应: {:?}", response);
        Ok(response.into_inner())
    }

    /// 切换分支
    pub async fn switch_branch(&mut self, branch_name: String) -> Result<SwitchBranchRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(SwitchBranchReq { branch_name });
        let response: tonic::Response<SwitchBranchRsp> = self.client.switch_branch(request).await?;
        println!("SwitchBranch 响应: {:?}", response);
        Ok(response.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::Duration;
    use std::sync::Once;

    static INIT: Once = Once::new();

    /// 启动边缘节点进程
    fn start_edge_daemon() -> Result<Child, Box<dyn std::error::Error>> {
        println!("正在启动边缘节点...");
        
        let mut child = Command::new("cargo")
            .args(&["run", "--bin", "crv-edge"])
            .current_dir("../crv-edge")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // 等待服务器启动
        println!("等待边缘节点启动...");
        thread::sleep(Duration::from_secs(3));

        // 检查进程是否还在运行
        match child.try_wait()? {
            Some(status) => {
                return Err(format!("边缘节点进程意外退出，状态: {:?}", status).into());
            }
            None => {
                println!("边缘节点启动成功！");
            }
        }

        Ok(child)
    }

    /// 停止边缘节点进程
    fn stop_edge_daemon(mut child: Child) {
        println!("正在停止边缘节点...");
        let _ = child.kill();
        let _ = child.wait();
        println!("边缘节点已停止");
    }

    #[tokio::test]
    async fn test_edge_daemon_integration() -> Result<(), Box<dyn std::error::Error>> {
        //只运行一次初始化
        // INIT.call_once(|| {
        //     println!("开始边缘节点集成测试");
        // });

        // // 启动边缘节点
        // let edge_process = start_edge_daemon()?;
        
        // // 确保在测试结束时停止进程
        // let _guard = EdgeDaemonGuard { process: Some(edge_process) };

        // // 等待服务器完全启动
        // thread::sleep(Duration::from_secs(2));


        // // 创建客户端
        // let server_addr = "http://127.0.0.1:34562";
        // let mut client = match CrvClient::new(server_addr).await {
        //     Ok(client) => {
        //         println!("✅ 客户端连接成功");
        //         client
        //     }
        //     Err(e) => {
        //         println!("❌ 客户端连接失败: {}", e);
        //         return Err(e);
        //     }
        // };

        // // 测试 1: Bonjour 指令
        // println!("\n🧪 测试 1: Bonjour 指令");
        // match client.bonjour().await {
        //     Ok(response) => {
        //         println!("✅ Bonjour 测试成功");
        //         println!("   守护进程版本: {}", response.daemon_version);
        //         println!("   API 级别: {}", response.api_level);
        //         println!("   平台: {}", response.platform);
        //         println!("   操作系统: {}", response.os);
        //         println!("   架构: {}", response.architecture);
        //     }
        //     Err(e) => {
        //         println!("❌ Bonjour 测试失败: {}", e);
        //         return Err(e);
        //     }
        // }

        // // 测试 2: CreateWorkspace 指令
        // println!("\n🧪 测试 2: CreateWorkspace 指令");
        // match client.create_workspace().await {
        //     Ok(response) => {
        //         if response.success {
        //             println!("✅ CreateWorkspace 测试成功");
        //             println!("   消息: {}", response.message);
        //             println!("   工作空间路径: {}", response.workspace_path);
        //         } else {
        //             println!("❌ CreateWorkspace 测试失败: {}", response.message);
        //             return Err(format!("创建工作空间失败: {}", response.message).into());
        //         }
        //     }
        //     Err(e) => {
        //         println!("❌ CreateWorkspace 测试失败: {}", e);
        //         return Err(e);
        //     }
        // }

        // // 测试 3: GetLatest 指令
        // println!("\n🧪 测试 3: GetLatest 指令");
        // match client.get_latest().await {
        //     Ok(response) => {
        //         if response.success {
        //             println!("✅ GetLatest 测试成功");
        //             println!("   消息: {}", response.message);
        //             println!("   文件数量: {}", response.file_paths.len());
        //             for (i, path) in response.file_paths.iter().enumerate() {
        //                 println!("   文件 {}: {}", i + 1, path);
        //             }
        //         } else {
        //             println!("❌ GetLatest 测试失败: {}", response.message);
        //             return Err(format!("获取最新文件失败: {}", response.message).into());
        //         }
        //     }
        //     Err(e) => {
        //         println!("❌ GetLatest 测试失败: {}", e);
        //         return Err(e);
        //     }
        // }

        // // 测试 4: Checkout 指令
        // println!("\n🧪 测试 4: Checkout 指令");
        // let test_file = "test_file.txt";
        // match client.checkout(test_file).await {
        //     Ok(response) => {
        //         if response.success {
        //             println!("✅ Checkout 测试成功");
        //             println!("   消息: {}", response.message);
        //             println!("   文件路径: {}", response.file_path);
        //         } else {
        //             println!("❌ Checkout 测试失败: {}", response.message);
        //             return Err(format!("检出文件失败: {}", response.message).into());
        //         }
        //     }
        //     Err(e) => {
        //         println!("❌ Checkout 测试失败: {}", e);
        //         return Err(e);
        //     }
        // }

        // // 测试 5: Summit 指令
        // println!("\n🧪 测试 5: Summit 指令");
        // match client.summit(test_file).await {
        //     Ok(response) => {
        //         if response.success {
        //             println!("✅ Summit 测试成功");
        //             println!("   消息: {}", response.message);
        //             println!("   文件路径: {}", response.file_path);
        //         } else {
        //             println!("❌ Summit 测试失败: {}", response.message);
        //             return Err(format!("提交文件失败: {}", response.message).into());
        //         }
        //     }
        //     Err(e) => {
        //         println!("❌ Summit 测试失败: {}", e);
        //         return Err(e);
        //     }
        // }

        // println!("\n🎉 所有测试通过！边缘节点集成测试成功完成。");
        Ok(())
    }

    /// 用于自动清理边缘节点进程的守护者
    struct EdgeDaemonGuard {
        process: Option<Child>,
    }

    impl Drop for EdgeDaemonGuard {
        fn drop(&mut self) {
            if let Some(process) = self.process.take() {
                stop_edge_daemon(process);
            }
        }
    }
}
