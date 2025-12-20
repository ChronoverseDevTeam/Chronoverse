use anyhow::Result;
use clap::{Parser, Subcommand};
use crv_edge::{
    daemon_server::config::BootstrapConfig,
    pb::{BonjourReq, system_service_client::SystemServiceClient},
};
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
            EdgeCommands::BootstrapConfig(bootstrap_config) => bootstrap_config.handle().await,
        }
    }
}

#[derive(Subcommand)]
pub enum EdgeCommands {
    Bonjour(BonjourCli),
    BootstrapConfig(BootstrapConfigCli),
}

#[derive(Parser)]
pub struct BonjourCli;

impl BonjourCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut system_client = SystemServiceClient::new(channel.clone());
        let response = system_client.bonjour(BonjourReq {}).await?;
        println!("{:?}", response.into_inner());
        Ok(())
    }
}

#[derive(Parser)]
#[command(about = "Show bootstrap config.", long_about = None)]
pub struct BootstrapConfigCli;

impl BootstrapConfigCli {
    pub async fn handle(&self) -> Result<()> {
        let bootstrap_config = BootstrapConfig::load().expect("Can't load bootstrap config.");
        println!("daemon_port:{}", bootstrap_config.daemon_port);
        println!(
            "embedded_database_root:{}",
            bootstrap_config.embedded_database_root
        );
        Ok(())
    }
}
