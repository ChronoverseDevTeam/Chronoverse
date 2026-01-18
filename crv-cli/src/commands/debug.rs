use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;
use crv_edge::pb::debug_service_client::DebugServiceClient;
use crv_edge::pb::{
    TransferBlueprintAsyncCheckReq, TransferBlueprintAsyncStartReq, TransferBlueprintReq,
    TransferBlueprintRsp,
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::time::sleep;
use tonic::transport::Channel;

#[derive(Parser)]
pub struct DebugCli {
    #[command(subcommand)]
    pub debug_commands: DebugCommands,
}

impl DebugCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        match &self.debug_commands {
            DebugCommands::TransferBlueprint(cmd) => cmd.handle(channel).await,
            DebugCommands::TransferBlueprintAsync(cmd) => cmd.handle(channel).await,
        }
    }
}

#[derive(Subcommand)]
pub enum DebugCommands {
    #[command(name = "transfer-blueprint")]
    TransferBlueprint(TransferBlueprintCli),
    #[command(name = "transfer-blueprint-async")]
    TransferBlueprintAsync(TransferBlueprintAsyncCli),
}

#[derive(Parser)]
pub struct TransferBlueprintCli {
    #[arg(long, default_value = "5")]
    worker_count: i32,
}

impl TransferBlueprintCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = DebugServiceClient::new(channel.clone());
        let request = TransferBlueprintReq {
            worker_count: self.worker_count,
        };

        println!(
            "{}",
            style("Starting Transfer Blueprint (Stream)...")
                .bold()
                .cyan()
        );

        let mut stream = client.transfer_blueprint(request).await?.into_inner();
        let m = MultiProgress::new();
        let mut bars: HashMap<String, ProgressBar> = HashMap::new();
        let progress_style = ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap()
        .progress_chars("##-");

        while let Some(msg) = stream.message().await? {
            update_progress(&m, &mut bars, &progress_style, msg);
        }

        m.clear().unwrap();
        println!("{}", style("Transfer Blueprint Completed!").bold().green());
        Ok(())
    }
}

#[derive(Parser)]
pub struct TransferBlueprintAsyncCli {
    #[arg(long, default_value = "5")]
    worker_count: i32,
}

impl TransferBlueprintAsyncCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = DebugServiceClient::new(channel.clone());

        // 1. Start Job
        println!(
            "{}",
            style("Starting Transfer Blueprint (Async)...")
                .bold()
                .cyan()
        );
        let start_resp = client
            .transfer_blueprint_async_start(TransferBlueprintAsyncStartReq {
                worker_count: self.worker_count,
            })
            .await?
            .into_inner();

        let job_id = start_resp.job_id;
        println!("Job Started: {}", style(&job_id).yellow());

        // 2. Poll Status
        let m = MultiProgress::new();
        let mut bars: HashMap<String, ProgressBar> = HashMap::new();
        let progress_style = ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
        )
        .unwrap()
        .progress_chars("##-");

        loop {
            let check_resp = client
                .transfer_blueprint_async_check(TransferBlueprintAsyncCheckReq {
                    job_id: job_id.clone(),
                })
                .await?
                .into_inner();

            for msg in check_resp.messages {
                update_progress(&m, &mut bars, &progress_style, msg);
            }

            if check_resp.job_status == "Completed" {
                m.clear().unwrap();
                println!("{}", style("Job Completed Successfully!").bold().green());
                break;
            } else if check_resp.job_status == "Failed" {
                m.clear().unwrap();
                println!("{}", style("Job Failed!").bold().red());
                break;
            }

            sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }
}

fn update_progress(
    m: &MultiProgress,
    bars: &mut HashMap<String, ProgressBar>,
    progress_style: &ProgressStyle,
    msg: TransferBlueprintRsp,
) {
    let worker_id = msg.worker_id;
    let pb = bars.entry(worker_id.clone()).or_insert_with(|| {
        let pb = m.add(ProgressBar::new(10)); // Total chunks is 10
        pb.set_style(progress_style.clone());
        pb.set_message(format!("Worker {}", worker_id));
        pb
    });

    let is_done = msg.message.contains("Chunk 10/10");

    match msg.r#type.as_str() {
        "progress" => {
            // Parse "Chunk x/y uploaded" to get position
            // Assuming format "Chunk {x}/{total} uploaded"
            if let Some(pos_str) = msg.message.split_whitespace().nth(1) {
                if let Some(current) = pos_str.split('/').next() {
                    if let Ok(pos) = current.parse::<u64>() {
                        pb.set_position(pos);
                    }
                }
            }
            pb.set_message(msg.message);
        }
        "warning" => {
            pb.set_message(format!("{} {}", style("WARN:").yellow(), msg.message));
        }
        "error" => {
            pb.set_message(format!("{} {}", style("ERR:").red(), msg.message));
        }
        _ => {
            pb.set_message(msg.message);
        }
    }

    if is_done {
        pb.finish_with_message("Done");
    }
}
