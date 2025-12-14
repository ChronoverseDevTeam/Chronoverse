mod commands;
mod logic;

use anyhow::{Context, Result};
use clap::Parser;
use commands::Cli;
use crv_edge::daemon_server::config::BootstrapConfig;
use tonic::transport::Endpoint;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 加载启动配置
    let bootstrap_config = BootstrapConfig::load().expect("Can't load bootstrap config.");

    // 连接到 Daemon
    let daemon_url = format!("127.0.0.1:{}", bootstrap_config.daemon_port);
    let channel = Endpoint::from_shared(daemon_url.clone())?
        .connect()
        .await
        .context(format!(
            "Failed to connect to edge. Is it running on {}?",
            daemon_url
        ))?;

    cli.handle(&channel).await?;

    Ok(())
}
