use std::collections::BTreeMap;

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::{Emoji, style};
use crv_edge::{
    daemon_server::config::BootstrapConfig,
    pb::{BonjourReq, GetRuntimeConfigReq, system_service_client::SystemServiceClient},
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
            EdgeCommands::RuntimeConfig(runtime_config) => runtime_config.handle(channel).await,
        }
    }
}

#[derive(Subcommand)]
pub enum EdgeCommands {
    Bonjour(BonjourCli),
    BootstrapConfig(BootstrapConfigCli),
    RuntimeConfig(RuntimeConfigCli),
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

        let mut settings = BTreeMap::new();
        settings.insert("daemon_port", format!("{}", bootstrap_config.daemon_port));
        settings.insert(
            "embedded_database_root",
            bootstrap_config.embedded_database_root.to_string(),
        );

        println!(
            "\n{}\n",
            style(" ⚙  Configuration Details ").bold().reverse().cyan()
        );
        // 1. 展示路径部分
        println!("{}", style("CONFIG LOCATION").bold().dim());
        println!(
            "{}\n",
            style(
                confy::get_configuration_file_path(
                    BootstrapConfig::CONFY_APP_NAME,
                    BootstrapConfig::CONFY_CONFIG_NAME
                )
                .unwrap()
                .to_string_lossy()
            )
            .underlined()
            .bright()
            .black()
        );

        // 2. 展示设置项部分
        println!("{}", style("SETTINGS").bold().dim());

        // 计算 Key 的最大长度以实现对齐
        let max_key_len = settings.keys().map(|k| k.len()).max().unwrap_or(0);

        for (key, value) in settings {
            println!(
                " {} {:width$} {} {}",
                Emoji("●", "*"),       // 前缀小圆点
                style(key).cyan(),     // Key 颜色
                style("→").dim(),      // 箭头符号
                style(value).yellow(), // Value 颜色
                width = max_key_len    // 动态填充宽度
            );
        }

        Ok(())
    }
}

#[derive(Parser)]
#[command(about = "Show runtime config.", long_about = None)]
pub struct RuntimeConfigCli;

impl RuntimeConfigCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = SystemServiceClient::new(channel.clone());
        let runtime_config = client
            .get_runtime_config(GetRuntimeConfigReq {})
            .await?
            .into_inner();

        let mut settings = BTreeMap::new();
        if let Some(item) = runtime_config.remote_addr {
            settings.insert("HIVE_PORT", format!("\"{}\"({})", item.value, item.source));
        }
        if let Some(item) = runtime_config.editor {
            settings.insert(
                "EDITOR_COMMAND",
                format!("\"{}\"({})", item.value, item.source),
            );
        }
        if let Some(item) = runtime_config.user {
            settings.insert("USER", format!("\"{}\"({})", item.value, item.source));
        }

        println!(
            "\n{}\n",
            style(" ⚙  Configuration Details ").bold().reverse().cyan()
        );

        // 1. 展示设置项部分
        println!("{}", style("SETTINGS").bold().dim());

        // 计算 Key 的最大长度以实现对齐
        let max_key_len = settings.keys().map(|k| k.len()).max().unwrap_or(0);

        for (key, value) in settings {
            println!(
                " {} {:width$} {} {}",
                Emoji("●", "*"),       // 前缀小圆点
                style(key).cyan(),     // Key 颜色
                style("→").dim(),      // 箭头符号
                style(value).yellow(), // Value 颜色
                width = max_key_len    // 动态填充宽度
            );
        }

        Ok(())
    }
}
