use clap::{Parser, Subcommand};
use crv_cli::client::CrvClient;

#[derive(Parser)]
#[command(about = "Chronoverse CLI - Command line interface for crv")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 连接到 crv-edge 守护进程
    Connect {
        /// 服务器地址 (例如: http://127.0.0.1:34562)
        #[arg(short, long, default_value = "http://127.0.0.1:34562")]
        server: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Connect { server } => {
            println!("正在连接到服务器: {}", server);

            let mut client = CrvClient::new(&server).await?;
            println!("连接成功！");

            let response = client.bonjour().await?;
            println!("收到服务器信息:");
            println!("  守护进程版本: {}", response.daemon_version);
            println!("  API 级别: {}", response.api_level);
            println!("  平台: {}", response.platform);
            println!("  操作系统: {}", response.os);
            println!("  架构: {}", response.architecture);
            println!("消息发送成功！");
        }
    }

    Ok(())
}
