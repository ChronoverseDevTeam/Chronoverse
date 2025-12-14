use anyhow::Result;
use clap::{Parser, Subcommand};
use tonic::transport::Channel;

#[derive(Parser)]
pub struct ChangelistCli {
    #[command(subcommand)]
    pub changelist_commands: ChangelistCommands,
}

impl ChangelistCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        match &self.changelist_commands {
            ChangelistCommands::Create(create_cli) => create_cli.handle(channel).await,
            ChangelistCommands::Delete(delete_cli) => delete_cli.handle(channel).await,
            ChangelistCommands::List(list_cli) => list_cli.handle(channel).await,
            ChangelistCommands::Describe(describe_cli) => describe_cli.handle(channel).await,
            ChangelistCommands::Append(append_cli) => append_cli.handle(channel).await,
            ChangelistCommands::Submit(submit_cli) => submit_cli.handle(channel).await,
        }
    }
}

#[derive(Subcommand)]
pub enum ChangelistCommands {
    Create(CreateCli),
    Delete(DeleteCli),
    List(ListCli),
    Describe(DescribeCli),
    Append(AppendCli),
    Submit(SubmitCli),
}

#[derive(Parser)]
pub struct CreateCli;

impl CreateCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
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

#[derive(Parser)]
pub struct AppendCli;

impl AppendCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct SubmitCli;

impl SubmitCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}
