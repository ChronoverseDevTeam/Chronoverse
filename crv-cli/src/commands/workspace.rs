use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;
use crv_edge::pb::{
    CreateWorkspaceReq, GetRuntimeConfigReq, system_service_client::SystemServiceClient,
    workspace_service_client::WorkspaceServiceClient,
};
use dialoguer::{Input, theme::ColorfulTheme};
use std::io::Write;
use tonic::{Request, transport::Channel};

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
        let mut system_client = SystemServiceClient::new(channel.clone());
        let runtime_config = system_client
            .get_runtime_config(GetRuntimeConfigReq {})
            .await?; // todo use editor set in it.

        // Step 1: Enter workspace name
        let workspace_name = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Workspace name")
            .interact_text()
            .expect("Meet error");

        if workspace_name.trim().is_empty() {
            anyhow::bail!("Workspace name cannot be empty");
        }

        // Step 2: Enter workspace root path with completion
        let workspace_root = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt("Workspace root path")
            .completion_with(&PathCompletion)
            .interact_text()
            .expect("Meet error");

        if workspace_root.trim().is_empty() {
            anyhow::bail!("Workspace root path cannot be empty");
        }

        // Step 3: Enter workspace mapping in editor
        let mapping = edit::edit(
            "# Enter workspace mapping view here\n# Lines starting with # will be ignored\n",
        )
        .expect("Meet error");

        // Process the mapping content (remove comment lines)
        let mapping = mapping
            .lines()
            .filter(|line| !line.trim().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        // Display summary
        println!(
            "\n{}",
            style("Workspace Configuration Summary:").bold().green()
        );
        println!("  Name: {}", style(&workspace_name).cyan());
        println!("  Root: {}", style(&workspace_root).cyan());
        println!("  Mapping: {} lines", style(mapping.lines().count()).cyan());

        let create_req = CreateWorkspaceReq {
            workspace_name: workspace_name,
            workspace_root: workspace_root,
            workspace_mapping: mapping,
        };
        let workspace_client = WorkspaceServiceClient::new(channel.clone());
        workspace_client.create_workspace(create_req).await?;

        println!(
            "\n{}",
            style("✓ Workspace creation prepared (gRPC call not implemented yet)").green()
        );

        Ok(())
    }
}

// Path completion helper
struct PathCompletion;

impl dialoguer::Completion for PathCompletion {
    fn get(&self, input: &str) -> Option<String> {
        use std::path::Path;

        let path = Path::new(input);
        let (dir, prefix) = if input.ends_with(std::path::MAIN_SEPARATOR) || input.is_empty() {
            (path, "")
        } else {
            (path.parent()?, path.file_name()?.to_str()?)
        };

        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };

        let entries = std::fs::read_dir(dir).ok()?;

        let mut matches: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                if name.starts_with(prefix) {
                    let full_path = if dir == Path::new(".") {
                        name.clone()
                    } else {
                        dir.join(&name).to_str()?.to_string()
                    };

                    // Add separator for directories
                    if e.file_type().ok()?.is_dir() {
                        Some(full_path + std::path::MAIN_SEPARATOR_STR)
                    } else {
                        Some(full_path)
                    }
                } else {
                    None
                }
            })
            .collect();

        matches.sort();
        matches.first().cloned()
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
