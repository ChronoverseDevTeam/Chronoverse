use clap::{Parser, Subcommand};

use crate::commands::workspace;

mod commands;

#[derive(Parser)]
#[command(about = "Chronoverse CLI - Command line interface for crv")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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

    match cli.command {
        Commands::Edge(edge) => commands::edge::handle(edge).await?,
        Commands::Workspace(workspace) => workspace::handle(workspace),
    }

    Ok(())
}
