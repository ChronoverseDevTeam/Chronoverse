use clap::{Parser, Subcommand};

#[derive(Parser)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommands,
}

#[derive(Subcommand)]
pub enum WorkspaceCommands {
    /// 创建工作区
    Create,
    /// 删除工作区
    Delete {
        /// 工作区名称
        name: String,
    },
    /// 列出工作区
    List,
    /// 描述工作区
    Describe {
        /// 工作区名称
        name: String,
    },
}

pub fn handle(args: WorkspaceArgs) {
    match args.command {
        WorkspaceCommands::Create => {
            todo!()
        }
        WorkspaceCommands::Delete { name } => {
            todo!()
        }
        WorkspaceCommands::List => {
            todo!()
        }
        WorkspaceCommands::Describe { name } => {
            todo!()
        }
    }
}
