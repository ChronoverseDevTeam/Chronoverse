use tonic::transport::Channel;
use crate::pb::edge_daemon_service_client::EdgeDaemonServiceClient;
use crate::pb::{BonjourReq, BonjourRsp, GetLatestReq, GetLatestRsp, CheckoutReq, CheckoutRsp, SummitReq, SummitRsp, CreateWorkspaceReq, CreateWorkspaceRsp};

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
        INIT.call_once(|| {
            println!("开始边缘节点集成测试");
        });

        // 启动边缘节点
        let edge_process = start_edge_daemon()?;
        
        // 确保在测试结束时停止进程
        let _guard = EdgeDaemonGuard { process: Some(edge_process) };

        // 等待服务器完全启动
        thread::sleep(Duration::from_secs(2));


        // 创建客户端
        let server_addr = "http://127.0.0.1:34562";
        let mut client = match CrvClient::new(server_addr).await {
            Ok(client) => {
                println!("✅ 客户端连接成功");
                client
            }
            Err(e) => {
                println!("❌ 客户端连接失败: {}", e);
                return Err(e);
            }
        };

        // 测试 1: Bonjour 指令
        println!("\n🧪 测试 1: Bonjour 指令");
        match client.bonjour().await {
            Ok(response) => {
                println!("✅ Bonjour 测试成功");
                println!("   守护进程版本: {}", response.daemon_version);
                println!("   API 级别: {}", response.api_level);
                println!("   平台: {}", response.platform);
                println!("   操作系统: {}", response.os);
                println!("   架构: {}", response.architecture);
            }
            Err(e) => {
                println!("❌ Bonjour 测试失败: {}", e);
                return Err(e);
            }
        }

        // 测试 2: CreateWorkspace 指令
        println!("\n🧪 测试 2: CreateWorkspace 指令");
        match client.create_workspace().await {
            Ok(response) => {
                if response.success {
                    println!("✅ CreateWorkspace 测试成功");
                    println!("   消息: {}", response.message);
                    println!("   工作空间路径: {}", response.workspace_path);
                } else {
                    println!("❌ CreateWorkspace 测试失败: {}", response.message);
                    return Err(format!("创建工作空间失败: {}", response.message).into());
                }
            }
            Err(e) => {
                println!("❌ CreateWorkspace 测试失败: {}", e);
                return Err(e);
            }
        }

        // 测试 3: GetLatest 指令
        println!("\n🧪 测试 3: GetLatest 指令");
        match client.get_latest().await {
            Ok(response) => {
                if response.success {
                    println!("✅ GetLatest 测试成功");
                    println!("   消息: {}", response.message);
                    println!("   文件数量: {}", response.file_paths.len());
                    for (i, path) in response.file_paths.iter().enumerate() {
                        println!("   文件 {}: {}", i + 1, path);
                    }
                } else {
                    println!("❌ GetLatest 测试失败: {}", response.message);
                    return Err(format!("获取最新文件失败: {}", response.message).into());
                }
            }
            Err(e) => {
                println!("❌ GetLatest 测试失败: {}", e);
                return Err(e);
            }
        }

        // 测试 4: Checkout 指令
        println!("\n🧪 测试 4: Checkout 指令");
        let test_file = "test_file.txt";
        match client.checkout(test_file).await {
            Ok(response) => {
                if response.success {
                    println!("✅ Checkout 测试成功");
                    println!("   消息: {}", response.message);
                    println!("   文件路径: {}", response.file_path);
                } else {
                    println!("❌ Checkout 测试失败: {}", response.message);
                    return Err(format!("检出文件失败: {}", response.message).into());
                }
            }
            Err(e) => {
                println!("❌ Checkout 测试失败: {}", e);
                return Err(e);
            }
        }

        // 测试 5: Summit 指令
        println!("\n🧪 测试 5: Summit 指令");
        match client.summit(test_file).await {
            Ok(response) => {
                if response.success {
                    println!("✅ Summit 测试成功");
                    println!("   消息: {}", response.message);
                    println!("   文件路径: {}", response.file_path);
                } else {
                    println!("❌ Summit 测试失败: {}", response.message);
                    return Err(format!("提交文件失败: {}", response.message).into());
                }
            }
            Err(e) => {
                println!("❌ Summit 测试失败: {}", e);
                return Err(e);
            }
        }

        println!("\n🎉 所有测试通过！边缘节点集成测试成功完成。");
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
