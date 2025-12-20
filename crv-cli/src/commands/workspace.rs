use anyhow::Result;
use clap::{Parser, Subcommand};
use tonic::transport::Channel;

#[derive(Parser)]
pub struct WorkspaceCli {
    #[command(subcommand)]
    pub workspace_commands: WorkspaceCommands,
}

#[derive(Subcommand)]
pub enum WorkspaceCommands {
    Create(CreateCli),
    Delete(DeleteCli),
    List(ListCli),
    Describe(DescribeCli),
}

impl WorkspaceCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        match &self.workspace_commands {
            WorkspaceCommands::Create(cli) => cli.handle(channel).await,
            WorkspaceCommands::Delete(cli) => cli.handle(channel).await,
            WorkspaceCommands::List(cli) => cli.handle(channel).await,
            WorkspaceCommands::Describe(cli) => cli.handle(channel).await,
        }
    }
}

#[derive(Parser)]
pub struct CreateCli;

impl CreateCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        // step 1. enter workspace name
        // step 2. enter workspace root
        // step 3. enter workspace mapping
        todo!()
    }
}

#[derive(Parser)]
pub struct DeleteCli;

impl DeleteCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct ListCli;

impl ListCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct DescribeCli;

impl DescribeCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}
