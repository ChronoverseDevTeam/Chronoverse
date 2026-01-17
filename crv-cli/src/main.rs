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

    println!("{}", console::style("Welcome to CRV Edge Shell").bold().cyan());
    println!("Type 'exit' or 'quit' to leave, 'help' for commands.\n");

    // 2. 初始化 Rustyline 编辑器
    let mut rl = DefaultEditor::new()?;
    //let _ = rl.load_history("history.txt");

    // 3. 进入交互式循环
    loop {
        //rustyline暂时没法debug（报错Permission denied），先使用标准输入输出
        print!("crv> ");
        io::stdout().flush().unwrap();
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input = input.trim().to_string();

        if let Some(args) = shlex::split(&input) {
            // 调用处理函数
            if let Err(e) = handle_repl_command(&args, &channel).await {
                eprintln!("{}: {}", console::style("Error").red(), e);
            }
        }
        // match input {
        //     Ok(line) => {
        //         let line = line.trim();
        //         if line.is_empty() { continue; }
                
        //         // 添加到历史记录
        //         let _ = rl.add_history_entry(line);

        //         // 处理退出指令
        //         if line == "exit" || line == "quit" {
        //             break;
        //         }

        //         // 4. 解析并执行命令
        //         // 我们使用 shlex 将字符串转为 Vec<String>，模拟标准命令行参数
        //         if let Some(args) = shlex::split(line) {
        //             // 调用处理函数
        //             if let Err(e) = handle_repl_command(&args, &channel).await {
        //                 eprintln!("{}: {}", console::style("Error").red(), e);
        //             }
        //         }
        //     }
        //     Err(ReadlineError::Interrupted) => { // Ctrl-C
        //         println!("CTRL-C");
        //         break;
        //     }
        //     Err(ReadlineError::Eof) => { // Ctrl-D
        //         println!("CTRL-D");
        //         break;
        //     }
        //     Err(err) => {
        //         println!("Error: {:?}", err);
        //         break;
        //     }
        // }
    }

    //let _ = rl.save_history("history.txt");
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