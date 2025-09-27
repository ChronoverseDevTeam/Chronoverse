use clap::{Parser, Subcommand};
use crv_cli::client::CrvClient;

#[derive(Parser)]
#[command(name = "crv-cli")]
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
        /// 要发送的消息
        #[arg(short, long, default_value = "Hello from crv-cli!")]
        message: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Connect { server, message } => {
            println!("正在连接到服务器: {}", server);
            
            let mut client = CrvClient::new(&server).await?;
            println!("连接成功！");
            
            println!("发送消息: {}", message);
            client.greeting(&message).await?;
            println!("消息发送成功！");
        }
    }

    Ok(())
}
