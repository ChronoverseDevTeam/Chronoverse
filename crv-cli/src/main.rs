mod commands;
mod logic;

use anyhow::Result;
use clap::Parser;
use commands::{Cli}; // 假设 WorkspaceCli 在这里
use crv_edge::daemon_server::config::BootstrapConfig;
use tonic::transport::Endpoint;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 加载配置和建立连接 (只需连接一次，Channel 是可以复用的)
    let bootstrap_config = BootstrapConfig::load().expect("Can't load bootstrap config.");
    let daemon_url = format!("http://[::1]:{}", bootstrap_config.daemon_port);
    let channel = Endpoint::from_shared(daemon_url.clone())?.connect_lazy();

    // 2. 检查参数决定模式
    let args: Vec<String> = std::env::args().collect();
    // 仅当没有参数或参数为 --repl 时进入 REPL 模式
    if args.len() == 1 || (args.len() == 2 && args[1] == "--repl") {
        run_repl(channel).await?;
    } else {
        // 直接执行命令
        let cli = Cli::parse();
        cli.handle(&channel).await?;
    }

    Ok(())
}

async fn run_repl(channel: tonic::transport::Channel) -> Result<()> {
    println!("{}", console::style("Welcome to CRV Edge Shell").bold().cyan());
    println!("Type 'exit' or 'quit' to leave, 'help' for commands.\n");

    // 2. 初始化 Rustyline 编辑器
    // let mut rl = DefaultEditor::new()?;
    // let _ = rl.load_history("history.txt");

    // 3. 进入交互式循环
    loop {
        // rustyline暂时没法debug（报错Permission denied），先使用标准输入输出
        print!("crv> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        let input_str = input.trim();
        if input_str.is_empty() {
            continue;
        }

        if input_str == "exit" || input_str == "quit" {
            break;
        }

        if let Some(args) = shlex::split(input_str) {
            // 调用处理函数
            if let Err(e) = handle_repl_command(&args, &channel).await {
                eprintln!("{}: {}", console::style("Error").red(), e);
            }
        }
    }
    // let _ = rl.save_history("history.txt");
    Ok(())
}

/// 解析并处理 REPL 中的单条命令
async fn handle_repl_command(args: &[String], channel: &tonic::transport::Channel) -> Result<()> {
    // 注意：clap 的 try_parse_from 第一个参数通常是程序名，所以我们需要在开头插入一个占位符
    let mut full_args = vec!["crv-shell".to_string()];
    full_args.extend_from_slice(args);

    // 使用 try_parse_from 而不是 parse，防止解析失败时直接 panic 退出程序
    match Cli::try_parse_from(full_args) {
        Ok(cli) => {
            cli.handle(channel).await?;
        }
        Err(e) => {
            // 如果是 help 或解析错误，clap 会生成美观的错误提示，直接打印即可
            e.print()?;
        }
    }
    Ok(())
}
