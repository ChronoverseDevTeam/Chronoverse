mod commands;
mod logic;

use anyhow::Result;
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
    let daemon_url = format!("http://[::1]:{}", bootstrap_config.daemon_port);
    let channel = Endpoint::from_shared(daemon_url.clone())?.connect_lazy();

    cli.handle(&channel).await?;

    Ok(())
}
