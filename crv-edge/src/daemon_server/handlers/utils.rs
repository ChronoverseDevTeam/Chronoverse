use crate::daemon_server::db::file::FileLocation;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crv_core::path::basic::{LocalDir, LocalPath, PathError, WorkspaceDir, WorkspacePath};
use crv_core::path::engine::PathEngine;
use std::path::Path;
use tonic::Status;
use walkdir::WalkDir;

pub enum LocationUnion {
    LocalDir(LocalDir),
    LocalPath(LocalPath),
    WorkspaceDir(WorkspaceDir),
    WorkspacePath(WorkspacePath),
}

/// 规范化用户输入的路径，返回 LocationUnion。
///
/// 用户可以输入三种路径：
/// 1. WorkspacePath: //workspace/path/to/file
/// 2. WorkspaceDir: //workspace/dir/
/// 3. LocalPath: /absolute/path/to/file
/// 4. LocalDir: /absolute/path/to/dir/
///
/// strict 意味着该方法在处理本地路径的时候，通过其末尾是否有路径分隔符来区分文件和路径。
/// 这意味着请求方必须对这一切有所把握（例如给予用户足够的提示）。
pub fn normalize_paths_strict(
    paths: &[String],
    path_engine: &PathEngine,
) -> AppResult<Vec<LocationUnion>> {
    // 处理 WorkspacePath
    let workspace_paths = paths
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
            path_engine
                .workspace_path_to_local_path(x)
                .ok_or(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not under current workspace.",
                    x.to_custom_string()
                ))))?;
            Ok(LocationUnion::WorkspacePath(x.clone()))
        })
        .collect::<AppResult<Vec<LocationUnion>>>()?;

    // 处理 WorkspaceDir
    let workspace_dirs = paths
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
            path_engine
                .workspace_dir_to_local_dir(x)
                .ok_or(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not under current workspace.",
                    x.to_custom_string()
                ))))?;
            Ok(LocationUnion::WorkspaceDir(x.clone()))
        })
        .collect::<AppResult<Vec<LocationUnion>>>()?;

    // 处理本地绝对路径
    let local_paths = paths
        .iter()
        .filter(|x| !x.starts_with("//") && !(x.ends_with("/") || x.ends_with("\\")))
        .map(|x| {
            LocalPath::parse(x).map_err(|e| {
                AppError::Raw(Status::invalid_argument(format!(
                    "Path {} format invalid.",
                    x
                )))
            })
        })
        .collect::<AppResult<Vec<LocalPath>>>()?
        .iter()
        .map(|x| {
            path_engine
                .local_path_to_workspace_path(x)
                .ok_or(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not under current workspace.",
                    x.to_local_path_string()
                ))))?;
            Ok(LocationUnion::LocalPath(x.clone()))
        })
        .collect::<AppResult<Vec<LocationUnion>>>()?;

    let local_dirs = paths
        .iter()
        .filter(|x| !x.starts_with("//") && (x.ends_with("/") || x.ends_with("\\")))
        .map(|x| {
            LocalDir::parse(x).map_err(|e| {
                AppError::Raw(Status::invalid_argument(format!(
                    "Path {} format invalid.",
                    x
                )))
            })
        })
        .collect::<AppResult<Vec<LocalDir>>>()?
        .iter()
        .map(|x| {
            path_engine
                .local_dir_to_workspace_dir(x)
                .ok_or(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not under current workspace.",
                    x.to_local_path_string()
                ))))?;
            Ok(LocationUnion::LocalDir(x.clone()))
        })
        .collect::<AppResult<Vec<LocationUnion>>>()?;

    let mut result = vec![];
    result.extend(workspace_paths);
    result.extend(workspace_dirs);
    result.extend(local_paths);
    result.extend(local_dirs);
    Ok(result)
}

/// 递归遍历文件系统中的目录，将路径列表展开为文件列表，本地不存在的文件不会出现在结果中
pub fn expand_to_mapped_files_in_fs(
    paths: &[LocationUnion],
    path_engine: &PathEngine,
) -> Vec<FileLocation> {
    let file_from_dir = paths
        .iter()
        .filter_map(|x| match x {
            LocationUnion::LocalDir(local_dir) => Some(local_dir.clone()),
            LocationUnion::LocalPath(_) => None,
            LocationUnion::WorkspaceDir(workspace_dir) => {
                path_engine.workspace_dir_to_local_dir(workspace_dir)
            }
            LocationUnion::WorkspacePath(_) => None,
        })
        .flat_map(|path| {
            WalkDir::new(path.to_local_path_string())
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| e.path().to_string_lossy().into_owned())
        })
        .map(|file_path| LocalPath::parse(&file_path).unwrap())
        .filter_map(|local_path| {
            let workspace_path = path_engine.local_path_to_workspace_path(&local_path)?;
            let depot_path = path_engine.mapping_local_path(&local_path)?;
            Some(FileLocation {
                local_path,
                workspace_path,
                depot_path,
            })
        })
        .collect::<Vec<_>>();

    let file_from_path = paths
        .iter()
        .filter_map(|x| match x {
            LocationUnion::LocalDir(_) => None,
            LocationUnion::LocalPath(local_path) => {
                if Path::new(&local_path.to_local_path_string()).is_file() {
                    Some(local_path.clone())
                } else {
                    None
                }
            }
            LocationUnion::WorkspaceDir(_) => None,
            LocationUnion::WorkspacePath(workspace_path) => {
                let local_path = path_engine.workspace_path_to_local_path(workspace_path)?;
                if Path::new(&local_path.to_local_path_string()).is_file() {
                    Some(local_path)
                } else {
                    None
                }
            }
        })
        .filter_map(|local_path| {
            let workspace_path = path_engine.local_path_to_workspace_path(&local_path);
            let depot_path = path_engine.mapping_local_path(&local_path);
            if workspace_path.is_none() || depot_path.is_none() {
                return None;
            }
            Some(FileLocation {
                local_path,
                workspace_path: workspace_path.unwrap(),
                depot_path: depot_path.unwrap(),
            })
        })
        .collect::<Vec<_>>();
    let mut result = vec![];
    result.extend(file_from_dir);
    result.extend(file_from_path);
    result
}

/// 遍历当前工作区的活跃文件，将路径列表展开为文件列表，如果文件不是活跃文件，则不会出现在结果中
pub fn expand_to_mapped_files_active(
    paths: &[LocationUnion],
    path_engine: &PathEngine,
    app_state: AppState,
) -> AppResult<Vec<FileLocation>> {
    let workspace_dirs = paths
        .iter()
        .filter_map(|x| match x {
            LocationUnion::LocalDir(local_dir) => path_engine.local_dir_to_workspace_dir(local_dir),
            LocationUnion::LocalPath(_) => None,
            LocationUnion::WorkspaceDir(workspace_dir) => Some(workspace_dir.clone()),
            LocationUnion::WorkspacePath(_) => None,
        })
        .collect::<Vec<_>>();
    let mut active_files = vec![];
    for dir in workspace_dirs {
        active_files.extend(
            app_state
                .db
                .get_active_file_under_dir(&dir)?
                .iter()
                .map(|x| x.0.clone()),
        );
    }
    let workspace_paths = paths.iter().filter_map(|x| match x {
        LocationUnion::LocalDir(_) => None,
        LocationUnion::LocalPath(local_path) => {
            path_engine.local_path_to_workspace_path(local_path)
        }
        LocationUnion::WorkspaceDir(_) => None,
        LocationUnion::WorkspacePath(workspace_path) => Some(workspace_path.clone()),
    });

    for path in workspace_paths {
        if app_state.db.get_active_file_action(&path)?.is_some() {
            active_files.push(path);
        }
    }

    let result = active_files
        .iter()
        .filter_map(|x| {
            let local_path = path_engine.workspace_path_to_local_path(&x)?;
            let depot_path = path_engine.mapping_local_path(&local_path)?;
            Some(FileLocation {
                local_path,
                workspace_path: x.clone(),
                depot_path,
            })
        })
        .collect::<Vec<_>>();
    Ok(result)
}

/// 遍历当前工作区的文件元数据，将路径列表展开为文件列表，如果文件不在元数据内，则不会出现在结果中
pub fn expand_to_mapped_files_in_edge_meta(
    paths: &[LocationUnion],
    path_engine: &PathEngine,
    app_state: AppState,
) -> AppResult<Vec<FileLocation>> {
    let workspace_dirs = paths
        .iter()
        .filter_map(|x| match x {
            LocationUnion::LocalDir(local_dir) => path_engine.local_dir_to_workspace_dir(local_dir),
            LocationUnion::LocalPath(_) => None,
            LocationUnion::WorkspaceDir(workspace_dir) => Some(workspace_dir.clone()),
            LocationUnion::WorkspacePath(_) => None,
        })
        .collect::<Vec<_>>();
    let mut files = vec![];
    for dir in workspace_dirs {
        files.extend(
            app_state
                .db
                .get_file_meta_under_dir(&dir)?
                .iter()
                .map(|x| x.0.clone()),
        );
    }
    let workspace_paths = paths.iter().filter_map(|x| match x {
        LocationUnion::LocalDir(_) => None,
        LocationUnion::LocalPath(local_path) => {
            path_engine.local_path_to_workspace_path(local_path)
        }
        LocationUnion::WorkspaceDir(_) => None,
        LocationUnion::WorkspacePath(workspace_path) => Some(workspace_path.clone()),
    });

    for path in workspace_paths {
        if app_state.db.get_file_meta(&path)?.is_some() {
            files.push(path);
        }
    }

    let result = files
        .iter()
        .filter_map(|x| {
            let local_path = path_engine.workspace_path_to_local_path(&x)?;
            let depot_path = path_engine.mapping_local_path(&local_path)?;
            Some(FileLocation {
                local_path,
                workspace_path: x.clone(),
                depot_path,
            })
        })
        .collect::<Vec<_>>();
    Ok(result)
}
