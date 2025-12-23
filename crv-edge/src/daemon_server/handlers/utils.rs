use crate::daemon_server::error::{AppError, AppResult};
use crv_core::path::basic::{WorkspaceDir, WorkspacePath, PathError};
use crv_core::workspace::entity::WorkspaceConfig;
use std::path::Path;
use tonic::Status;
use walkdir::WalkDir;

/// 规范化用户输入的路径，返回本地绝对路径列表
/// 
/// 用户可以输入三种路径：
/// 1. WorkspacePath: //workspace/path/to/file
/// 2. WorkspaceDir: //workspace/dir/
/// 3. 本地绝对路径: /absolute/path/to/file
pub fn normalize_paths(
    paths: &[String],
    workspace_name: &str,
    workspace_config: &WorkspaceConfig,
) -> AppResult<Vec<String>> {
    // 处理 WorkspacePath
    let local_file_from_workspace_path = paths
        .iter()
        .filter(|x| x.starts_with("//") && !x.ends_with("/"))
        .map(|x| WorkspacePath::parse(x))
        .collect::<Result<Vec<WorkspacePath>, PathError>>()
        .map_err(|e| {
            AppError::Raw(Status::invalid_argument(format!(
                "Can't parse workspace path: {e}"
            )))
        })?
        .iter()
        .map(|x| {
            if x.workspace_name == workspace_name {
                Ok(x.clone())
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Workspace path {} is not under workspace {}.",
                    x.to_string(),
                    workspace_name
                ))))
            }
        })
        .collect::<AppResult<Vec<WorkspacePath>>>()?
        .iter()
        .map(|x| {
            let local_path = x
                .into_local_path(&workspace_config.root_dir)
                .to_local_path_string();
            if Path::new(&local_path).exists() {
                Ok(local_path)
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not exist.",
                    local_path
                ))))
            }
        })
        .collect::<AppResult<Vec<String>>>()?;

    // 处理 WorkspaceDir
    let local_dir_from_workspace_dir = paths
        .iter()
        .filter(|x| x.starts_with("//") && x.ends_with("/"))
        .map(|x| WorkspaceDir::parse(x))
        .collect::<Result<Vec<WorkspaceDir>, PathError>>()
        .map_err(|e| {
            AppError::Raw(Status::invalid_argument(format!(
                "Can't parse workspace dir: {e}"
            )))
        })?
        .iter()
        .map(|x| {
            if x.workspace_name == workspace_name {
                Ok(x.clone())
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Workspace path {} is not under workspace {}.",
                    x.to_string(),
                    workspace_name
                ))))
            }
        })
        .collect::<AppResult<Vec<WorkspaceDir>>>()?
        .iter()
        .map(|x| {
            let local_path = x
                .into_local_dir(&workspace_config.root_dir)
                .to_local_path_string();
            if Path::new(&local_path).exists() {
                Ok(local_path)
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not exist.",
                    local_path
                ))))
            }
        })
        .collect::<AppResult<Vec<String>>>()?;

    // 处理本地绝对路径
    let local_paths = paths
        .iter()
        .filter(|x| !x.starts_with("//"))
        .map(|x| {
            if Path::new(x).exists() {
                Ok(x.clone())
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not exist.",
                    x
                ))))
            }
        })
        .collect::<AppResult<Vec<String>>>()?
        .iter()
        .chain(local_dir_from_workspace_dir.iter())
        .chain(local_file_from_workspace_path.iter())
        .map(|x| x.clone())
        .collect::<Vec<String>>();

    Ok(local_paths)
}

/// 将路径列表展开为文件列表（递归遍历目录）
pub fn expand_paths_to_files(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .flat_map(|path| {
            WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| e.path().to_string_lossy().into_owned())
        })
        .collect()
}

