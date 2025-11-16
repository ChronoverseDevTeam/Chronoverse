use clap::{Parser, Subcommand};
use std::io::{self, Write};

mod commands;

#[derive(Parser)]
#[command(about = "Chronoverse CLI - Command line interface for crv")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// 服务器地址
    #[arg(short, long, default_value = "http://127.0.0.1:34562")]
    server: String,

    /// 使用本地模拟模式
    #[arg(short, long)]
    local: bool,

    /// 本地模拟：工作空间根目录
    #[arg(short = 'w', long, default_value = "./workspace")]
    workspace: String,

    /// 本地模拟：服务器根目录
    #[arg(long, default_value = "./server")]
    server_root: String,
}

#[derive(Subcommand)]
enum Commands {
    /// 管理 crv-edge 守护进程
    Edge(commands::edge::EdgeArgs),
    /// 管理工作区
    Workspace(commands::workspace::WorkspaceArgs),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    run_interactive_mode(cli).await
}

async fn run_interactive_mode(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    use crv_cli::client::CrvClient;

    // 初始化客户端（总是使用 gRPC 连接）
    println!("连接服务器: {}", cli.server);
    let mut client = CrvClient::new_grpc(&cli.server).await?;
    println!("连接成功");

    // 如果选择本地模拟模式，设置本地模拟参数
    if cli.local {
        println!("启用本地模拟模式");
        println!("工作空间: {}", cli.workspace);
        println!("服务器根: {}", cli.server_root);
        client.enable_local_simulation(&cli.workspace, &cli.server_root)?;
    }

    println!("输入 'help' 查看帮助，'exit' 退出\n");

    loop {
        print!("crv> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "exit" || input == "quit" {
            break;
        }

        if input == "help" {
            print_help(cli.local);
            continue;
        }

        // 解析命令
        let args: Vec<&str> = input.split_whitespace().collect();
        let mut full_args = vec!["crv"];
        full_args.extend(args);

        match Cli::try_parse_from(&full_args) {
            Ok(parsed) => {
                if let Some(command) = parsed.command {
                    match command {
                        Commands::Edge(edge_args) => {
                            if let Err(e) =
                                commands::edge::handle(edge_args.command, &mut client, cli.local)
                                    .await
                            {
                                eprintln!("错误: {}", e);
                            }
                        }
                        Commands::Workspace(ws_args) => {
                            commands::workspace::handle(ws_args);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
        println!();
    }

    Ok(())
}

fn print_help(is_local: bool) {
    println!("可用命令:");
    println!("  edge ping                          - 测试连接");
    println!("  edge create-workspace              - 创建工作空间");
    println!("  edge checkout <FILE>               - 检出文件");
    println!("  edge get-latest                    - 获取文件列表");
    if is_local {
        println!("  edge get-revision <FILE> -r <REV> - 切换版本");
    }
    println!("  edge submit <FILE> -d <DESC>       - 提交文件");
    println!("  workspace list                     - 列出工作区");
    println!("  help                               - 显示帮助");
    println!("  exit                               - 退出");
}
