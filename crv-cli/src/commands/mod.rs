mod changelist;
mod debug;
mod edge;
mod file;
mod workspace;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tonic::transport::Channel;

#[derive(Parser)]
#[command(name = "crv")]
#[command(about = "Command line interface for chronoverse", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        match &self.command {
            Commands::Edge(edge_cli) => edge_cli.handle(channel).await,
            Commands::Add(add_cli) => add_cli.handle(channel).await,
            Commands::Delete(delete_cli) => delete_cli.handle(channel).await,
            Commands::ListActiveFiles(list_cli) => list_cli.handle(channel).await,
            Commands::Sync(sync_cli) => sync_cli.handle(channel).await,
            Commands::Lock(lock_cli) => lock_cli.handle(channel).await,
            Commands::Submit(submit_cli) => submit_cli.handle(channel).await,
            Commands::Revert(revert_cli) => revert_cli.handle(channel).await,
            Commands::Workspace(workspace_cli) => workspace_cli.handle(channel).await,
            Commands::Changelist(changelist_cli) => changelist_cli.handle(channel).await,
            Commands::Debug(debug_cli) => debug_cli.handle(channel).await,
        }
    }
}

#[derive(Subcommand)]
pub enum Commands {
    Edge(edge::EdgeCli),
    Add(file::AddCli),
    Delete(file::DeleteCli),
    #[command(name = "showactive")]
    ListActiveFiles(file::ListActiveFilesCli),
    Sync(file::SyncCli),
    Lock(file::LockCli),
    Submit(file::SubmitCli),
    Revert(file::RevertCli),
    Workspace(workspace::WorkspaceCli),
    Changelist(changelist::ChangelistCli),
    Debug(debug::DebugCli),
}
