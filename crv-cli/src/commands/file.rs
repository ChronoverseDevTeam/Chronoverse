use std::process;

use anyhow::Result;
use clap::Parser;
use console::style;
use crv_edge::pb::{AddReq, DeleteReq, ListActiveFilesReq, SubmitReq, SyncReq, file_service_client::FileServiceClient, CheckoutReq};
use dialoguer::{Input, theme::ColorfulTheme};
use tokio::signal;
use tokio_stream::StreamExt;
use tonic::transport::Channel;

#[derive(Parser)]
pub struct AddCli {
    /// Workspace name
    #[arg(short, long)]
    pub workspace: String,

    /// Paths to add (can be local paths, workspace paths, or depot paths)
    #[arg(required = true)]
    pub paths: Vec<String>,
}

impl AddCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = FileServiceClient::new(channel.clone());

        println!("{}", style("Adding files...").cyan());

        let request = AddReq {
            workspace_name: self.workspace.clone(),
            paths: self.paths.clone(),
        };

        let response = client.add(request).await?.into_inner();

        let count = response.added_paths.len();
        for path in response.added_paths {
            println!("  {} {}", style("✓").green(), path);
        }

        println!(
            "{}",
            style(format!("Added {} file(s) successfully!", count)).green()
        );
        Ok(())
    }
}

#[derive(Parser)]
pub struct CheckoutCli {
    /// Workspace name
    #[arg(short, long)]
    pub workspace: String,

    /// Paths to checkout (can be local paths, workspace paths, or depot paths)
    #[arg(required = true)]
    pub paths: Vec<String>,
}

impl CheckoutCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = FileServiceClient::new(channel.clone());

        println!("{}", style("Checkout files...").cyan());

        let request = CheckoutReq {
            workspace_name: self.workspace.clone(),
            paths: self.paths.clone(),
        };

        let response = client.checkout(request).await?.into_inner();

        let count = response.checkouted_paths.len();
        for path in response.checkouted_paths {
            println!("  {} {}", style("✓").green(), path);
        }

        println!(
            "{}",
            style(format!("Checkout {} file(s) successfully!", count)).green()
        );
        Ok(())
    }
}


#[derive(Parser)]
pub struct SubmitCli {
    /// Workspace name
    #[arg(short, long)]
    pub workspace: String,

    /// Paths to submit (can be local paths, workspace paths, or depot paths)
    #[arg(required = true)]
    pub paths: Vec<String>,

    /// Submit description
    #[arg(short, long)]
    pub description: Option<String>,
}

impl SubmitCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = FileServiceClient::new(channel.clone());

        // Get description - either from argument or prompt
        let description = if let Some(desc) = &self.description {
            desc.clone()
        } else {
            Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter submit description")
                .interact_text()?
        };

        println!("{}", style("Submitting files...").cyan());

        let request = SubmitReq {
            workspace_name: self.workspace.clone(),
            paths: self.paths.clone(),
            description,
        };

        let mut stream = client.submit(request).await?.into_inner();

        // Spawn Ctrl+C handler
        tokio::spawn(async move {
            if let Ok(_) = signal::ctrl_c().await {
                println!("\n{}", style("Cancelling submit...").bold().yellow());
                process::exit(0);
            }
        });

        // Process the stream
        while let Some(progress) = stream.next().await {
            match progress {
                Ok(p) => {
                    println!(
                        "  {} {} (completed: {} bytes){}{}",
                        style("✓").green(),
                        p.path,
                        p.bytes_completed_so_far,
                        if !p.info.is_empty() {
                            format!("[info]{}.", p.info)
                        } else {
                            String::new()
                        },
                        if !p.warning.is_empty() {
                            format!("[warning]{}.", p.warning)
                        } else {
                            String::new()
                        }
                    );
                }
                Err(e) => {
                    eprintln!("{} {}", style("Error:").red(), e);
                    return Err(e.into());
                }
            }
        }

        println!("{}", style("Submit completed successfully!").green());
        Ok(())
    }
}

#[derive(Parser)]
pub struct SyncCli {
    /// Workspace name
    #[arg(short, long)]
    pub workspace: String,

    /// Paths to sync (can be local paths, workspace paths, or depot paths)
    #[arg(required = true)]
    pub paths: Vec<String>,

    /// Force sync even if files are already up to date
    #[arg(short, long, default_value = "false")]
    pub force: bool,
}

impl SyncCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = FileServiceClient::new(channel.clone());

        println!("{}", style("Syncing files...").cyan());

        let request = SyncReq {
            workspace_name: self.workspace.clone(),
            paths: self.paths.clone(),
            force: self.force,
        };

        let mut stream = client.sync(request).await?.into_inner();

        // Spawn Ctrl+C handler
        tokio::spawn(async move {
            if let Ok(_) = signal::ctrl_c().await {
                println!("\n{}", style("Cancelling sync...").bold().yellow());
                process::exit(0);
            }
        });

        // Process the stream
        while let Some(progress) = stream.next().await {
            match progress {
                Ok(p) => {
                    if let Some(payload) = p.payload {
                        use crv_edge::pb::sync_progress::Payload;
                        match payload {
                            Payload::Metadata(meta) => {
                                let total_files_to_sync = meta.files.len();
                                let total_bytes_to_sync =
                                    meta.files.iter().map(|x| x.size).sum::<i64>();
                                println!(
                                    "Total files to sync: {}, total size: {} bytes",
                                    total_files_to_sync, total_bytes_to_sync
                                );
                            }
                            Payload::FileUpdate(update) => {
                                let status_icon = if !update.warning.is_empty() {
                                    style("!").yellow()
                                } else {
                                    style("✓").green()
                                };

                                println!(
                                    "  {} {} [{}] (completed: {} bytes)",
                                    status_icon,
                                    update.path,
                                    update.info,
                                    update.bytes_completed_so_far
                                );

                                if !update.info.is_empty() {
                                    println!("    {}", style(update.info).dim());
                                }
                                if !update.warning.is_empty() {
                                    println!("    {}", style(update.warning).yellow());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{} {}", style("Error:").red(), e);
                    return Err(e.into());
                }
            }
        }

        println!("{}", style("Sync completed successfully!").green());
        Ok(())
    }
}

#[derive(Parser)]
pub struct DeleteCli {
    /// Workspace name
    #[arg(short, long)]
    pub workspace: String,

    /// Paths to delete (can be local paths, workspace paths, or depot paths)
    #[arg(required = true)]
    pub paths: Vec<String>,
}

impl DeleteCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = FileServiceClient::new(channel.clone());

        println!("{}", style("Marking files for deletion...").cyan());

        let request = DeleteReq {
            workspace_name: self.workspace.clone(),
            paths: self.paths.clone(),
        };

        let response = client.delete(request).await?.into_inner();

        let count = response.deleted_paths.len();
        for path in response.deleted_paths {
            println!("  {} {}", style("✓").green(), path);
        }

        println!(
            "{}",
            style(format!(
                "Marked {} file(s) for deletion successfully!",
                count
            ))
            .green()
        );
        Ok(())
    }
}

#[derive(Parser)]
pub struct RevertCli;

impl RevertCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct ListActiveFilesCli {
    /// Workspace name
    #[arg(short, long)]
    pub workspace: String,

    /// Directory path to list active files from (default: workspace root)
    #[arg(short, long, default_value = ".")]
    pub path: String,
}

impl ListActiveFilesCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = FileServiceClient::new(channel.clone());

        let request = ListActiveFilesReq {
            workspace_name: self.workspace.clone(),
            path: self.path.clone(),
        };

        let response = client.list_active_files(request).await?.into_inner();

        if response.active_files.is_empty() {
            println!("{}", style("No active files found.").yellow());
        } else {
            println!(
                "{}",
                style(format!(
                    "Found {} active file(s):",
                    response.active_files.len()
                ))
                .cyan()
            );
            println!();
            for file_info in response.active_files {
                let action_color = match file_info.action.as_str() {
                    "add" => style(&file_info.action).green(),
                    "edit" => style(&file_info.action).yellow(),
                    "delete" => style(&file_info.action).red(),
                    _ => style(&file_info.action).white(),
                };
                println!("  {} {} {}", action_color, style("|").dim(), file_info.path);
            }
        }

        Ok(())
    }
}

#[derive(Parser)]
pub struct LockCli;

impl LockCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}
