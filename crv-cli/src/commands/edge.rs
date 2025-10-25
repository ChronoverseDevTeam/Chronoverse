use clap::{Parser, Subcommand};
use crv_cli::client::CrvClient;

#[derive(Parser)]
pub struct EdgeArgs {
    #[command(subcommand)]
    pub command: EdgeCommands,
}

#[derive(Subcommand)]
pub enum EdgeCommands {
    /// 测试与 crv-edge 守护进程之间的连接
    Ping,
    /// 创建工作空间
    CreateWorkspace,
    /// 检出文件到本地工作空间
    Checkout {
        /// 文件路径 (depot path)
        #[arg(value_name = "FILE")]
        file_path: String,
    },
    /// 获取服务器上的最新文件列表
    GetLatest,
    /// 获取指定版本的文件（仅本地模拟模式）
    GetRevision {
        /// 文件路径
        #[arg(value_name = "FILE")]
        file_path: String,
        /// 版本号
        #[arg(short, long)]
        revision: u64,
    },
    /// 提交本地修改到服务器
    Submit {
        /// 文件路径
        #[arg(value_name = "FILE")]
        file_path: String,
        /// 提交描述
        #[arg(short, long)]
        description: String,
    },
}

/// 处理命令（使用已有的客户端连接）
pub async fn handle(
    command: EdgeCommands,
    client: &mut CrvClient,
    is_local: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        EdgeCommands::Ping => {
            let response = client.bonjour().await?;
            println!("收到服务器信息:");
            println!("  守护进程版本: {}", response.daemon_version);
            println!("  API 级别: {}", response.api_level);
            println!("  平台: {}", response.platform);
            println!("  操作系统: {}", response.os);
            println!("  架构: {}", response.architecture);
        }

        EdgeCommands::CreateWorkspace => {
            println!("正在创建工作空间...");
            let result = client.create_workspace().await?;
            if result.success {
                println!("✅ {}", result.message);
                println!("工作空间路径: {}", result.workspace_path);
            } else {
                println!("❌ {}", result.message);
            }
        }
        
        EdgeCommands::Checkout { file_path } => {
            println!("正在检出文件: {}", file_path);
            let result = client.checkout(&file_path).await?;
            println!("✅ {}", result);
        }

        EdgeCommands::GetLatest => {
            println!("正在获取最新文件列表...");
            let files = client.get_latest().await?;
            
            if files.is_empty() {
                println!("服务器上没有文件");
            } else {
                println!("服务器上的文件列表 ({} 个文件):", files.len());
                for (idx, file) in files.iter().enumerate() {
                    println!("  {}. {}", idx + 1, file);
                }
            }
        }

        EdgeCommands::GetRevision { file_path, revision } => {
            if !is_local {
                return Err("get-revision 仅在本地模拟模式下可用".into());
            }
            println!("正在切换到版本 {} of {}", revision, file_path);
            let result = client.change_revision(&file_path, revision)?;
            println!("✅ {}", result);
        }

        EdgeCommands::Submit { file_path, description } => {
            println!("正在提交文件: {}", file_path);
            println!("描述: {}", description);
            let result = client.submit(&file_path, description).await?;
            println!("✅ {}", result);
        }
    }

    Ok(())
}
