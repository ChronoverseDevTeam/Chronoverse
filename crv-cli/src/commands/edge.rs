use anyhow::Result;
use clap::{Parser, Subcommand};
use crv_edge::pb::{BonjourReq, system_service_client::SystemServiceClient};
use tonic::transport::Channel;

#[derive(Parser)]
// #[command(about = "Edge command.", long_about = None)]
pub struct EdgeCli {
    #[command(subcommand)]
    pub edge_commands: EdgeCommands,
}

impl EdgeCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        match &self.edge_commands {
            EdgeCommands::Bonjour(bonjour) => bonjour.handle(channel).await,
        }
    }
}

#[derive(Subcommand)]
pub enum EdgeCommands {
    Bonjour(BonjourCli),
}

#[derive(Parser)]
pub struct BonjourCli;

impl BonjourCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut system_client = SystemServiceClient::new(channel.clone());
        system_client.bonjour(BonjourReq {}).await?;
        Ok(())
    }
}
