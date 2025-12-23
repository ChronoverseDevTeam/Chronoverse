use anyhow::Result;
use clap::Parser;
use console::style;
use crv_edge::pb::{
    file_service_client::FileServiceClient, AddReq, DeleteReq, SubmitReq, SyncReq,
};
use dialoguer::{Input, theme::ColorfulTheme};
use tonic::transport::Channel;
use tokio_stream::StreamExt;

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
        
        println!("{}", style(format!("Added {} file(s) successfully!", count)).green());
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
        
        // Process the stream
        while let Some(progress) = stream.next().await {
            match progress {
                Ok(p) => {
                    println!(
                        "  {} {} (completed: {} bytes)",
                        style("✓").green(),
                        p.path,
                        p.bytes_completed_so_far
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
        
        // Process the stream
        while let Some(progress) = stream.next().await {
            match progress {
                Ok(p) => {
                    if let Some(payload) = p.payload {
                        use crv_edge::pb::sync_progress::Payload;
                        match payload {
                            Payload::Metadata(meta) => {
                                println!(
                                    "Total files to sync: {}, total size: {} bytes",
                                    meta.total_files_to_sync, meta.total_bytes_to_sync
                                );
                            }
                            Payload::FileUpdate(update) => {
                                println!(
                                    "  {} {} [{}] (completed: {} bytes)",
                                    style("✓").green(),
                                    update.path,
                                    update.action,
                                    update.bytes_completed_so_far
                                );
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
        
        println!("{}", style(format!("Marked {} file(s) for deletion successfully!", count)).green());
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
pub struct LockCli;

impl LockCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}
