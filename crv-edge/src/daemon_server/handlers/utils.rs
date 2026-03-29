use crate::daemon_server::db::file::FileLocation;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crv_core::path::basic::{
    DepotPath, FilenameWildcard, LocalPath, LocalPathWildcard, PathError, RangeDepotWildcard,
    WorkspaceDir, WorkspacePath, WorkspacePathWildcard,
};
use crv_core::path::engine::PathEngine;
use std::collections::HashSet;
use tonic::Status;
use walkdir::WalkDir;

pub enum LocationUnion {
    Local(LocalPathWildcard),
    Workspace(WorkspacePathWildcard),
    Depot(RangeDepotWildcard),
}

/// 规范化用户输入的路径，返回 LocationUnion。
///
/// 用户输入的路径可能是 Local、Workspace、Depot 三种。
///
/// Workspace 和 Depot 的语法相同，优先按照 Workspace 进行解析。
pub fn normalize_path(path: &str, path_engine: &PathEngine) -> AppResult<LocationUnion> {
    return if path.starts_with("//") {
        // 先尝试按照 WorkspacePathWildcard 进行解析，如果发生错误，再按照 Depot 进行解析
        let workspace_path_wildcard = WorkspacePathWildcard::parse(path);

        if let Ok(workspace_path_wildcard) = workspace_path_wildcard {
            if workspace_path_wildcard.workspace_name == path_engine.workspace_name {
                return Ok(LocationUnion::Workspace(workspace_path_wildcard));
            }
        }

        let range_depot_wildcard = RangeDepotWildcard::parse(path).map_err(|e| {
            AppError::Raw(Status::invalid_argument(format!(
                "Can't parse depot/workspace path: {e}"
            )))
        })?;

        Ok(LocationUnion::Depot(range_depot_wildcard))
    } else {
        let local_path_wildcard = LocalPathWildcard::parse(path).map_err(|e| {
            AppError::Raw(Status::invalid_argument(format!(
                "Can't parse local path: {e}"
            )))
        })?;

        Ok(LocationUnion::Local(local_path_wildcard))
    };
}

/// 递归遍历文件系统中的目录，将路径展开为文件列表，本地不存在的文件不会出现在结果中，
/// 没有映射到当前 workspace 的路径也不会出现在结果中
pub fn expand_to_mapped_files_in_fs(
    path: &LocationUnion,
    path_engine: &PathEngine,
) -> Vec<FileLocation> {
    // depot_filter.is_some() 意味着用户传入的是 depot wildcard，
    // 展开后可能出现部分文件不在该 depot wildcard 描述范围内的情况，需要执行一次后置的过滤操作
    let (local_wildcard_candidates, depot_filter) = match path {
        LocationUnion::Local(local_wildcard) => (vec![local_wildcard.clone()], None),
        LocationUnion::Workspace(workspace_wildcard) => {
            let local_dirs = path_engine.workspace_dir_to_local_dir(&WorkspaceDir {
                workspace_name: workspace_wildcard.workspace_name.clone(),
                dirs: workspace_wildcard.dirs.clone(),
            });

            if let Some(local_dirs) = local_dirs {
                let local_wildcard = LocalPathWildcard {
                    dirs: local_dirs,
                    recursive: workspace_wildcard.recursive,
                    wildcard: workspace_wildcard.wildcard.clone(),
                };
                (vec![local_wildcard], None)
            } else {
                (vec![], None)
            }
        }
        LocationUnion::Depot(depot_wildcard) => {
            let local_wildcards = path_engine.depot_wildcard_to_local_candidate(&depot_wildcard);
            (local_wildcards, Some(depot_wildcard))
        }
    };

    // 将 local_wildcard_candidates 展开为本地文件系统中的文件，并合成 Vec<FileLocation>
    let mut result = vec![];

    for wildcard in local_wildcard_candidates {
        let mut files = vec![];
        if wildcard.recursive {
            for entry in WalkDir::new(wildcard.dirs.to_local_path_string()) {
                let entry = entry.unwrap();
                files.push(entry);
            }
        } else {
            for entry in WalkDir::new(wildcard.dirs.to_local_path_string()).max_depth(1) {
                let entry = entry.unwrap();
                files.push(entry);
            }
        }

        for file in files {
            // 按后缀名进行过滤
            match &wildcard.wildcard {
                FilenameWildcard::Exact(filename) => {
                    if file.file_name().to_str() != Some(filename) {
                        continue;
                    }
                }
                FilenameWildcard::Extension(extension_name) => {
                    if let Some(filename) = file.file_name().to_str() {
                        if !filename.ends_with(extension_name) {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                FilenameWildcard::All => {}
            }

            let file_path = file.path().to_str();
            if file_path.is_none() {
                println!("Path {:?} can't be convert to str, skip it.", file.path());
                continue;
            }

            let file_path = file_path.unwrap();
            let local_path = LocalPath::parse(file_path);

            if local_path.is_err() {
                println!(
                    "Path {file_path} can't be converted to LocalPath which indicate bugs in parser."
                );
                continue;
            }

            let local_path = local_path.unwrap();
            let workspace_path = path_engine.local_path_to_workspace_path(&local_path);
            let depot_path = path_engine.mapping_local_path(&local_path);
            if workspace_path.is_none() || depot_path.is_none() {
                continue;
            }

            let workspace_path = workspace_path.unwrap();
            let depot_path = depot_path.unwrap();

            // 如果 depot_filter.is_some()，执行后置过滤操作
            if let Some(depot_filter) = depot_filter {
                if depot_filter.match_and_get_diff(&depot_path).is_none() {
                    continue;
                }
            }

            result.push(FileLocation {
                local_path,
                workspace_path,
                depot_path,
            });
        }
    }

    return result;
}

/// 遍历当前工作区的活跃文件，将路径列表展开为文件列表，如果文件不是活跃文件，则不会出现在结果中
pub fn expand_to_mapped_files_active(
    paths: &LocationUnion,
    path_engine: &PathEngine,
    app_state: AppState,
) -> AppResult<Vec<FileLocation>> {
    todo!()
}

/// 遍历当前工作区的文件元数据，将路径列表展开为文件列表，如果文件不在元数据内，则不会出现在结果中
pub fn expand_to_mapped_files_in_edge_meta(
    paths: &LocationUnion,
    path_engine: &PathEngine,
    app_state: AppState,
) -> AppResult<Vec<FileLocation>> {
    todo!()
}

/// 将给定 depot path 列表转化为能够被 filter 命中的、映射到当前工作区的 FileLocation。
/// 如果没有被 filter 命中，或者不在当前工作区的映射下，不会出现在结果中。
///
/// 注意，结果可能存在重复项目。
pub fn filter_depot_paths(
    filter: &LocationUnion,
    depot_paths: &[DepotPath],
    path_engine: &PathEngine,
) -> Vec<FileLocation> {
    // local_filter.is_some() 意味着用户传入的是 local/workspace wildcard，
    // 转化后可能出现部分文件不在该 wildcard 描述范围内的情况，需要执行一次后置的过滤操作
    let (depot_wildcard_candidates, local_filter) = match filter {
        LocationUnion::Local(local_wildcard) => {
            let depot_wildcards = path_engine.local_wildcard_to_depot_candidate(&local_wildcard);
            (depot_wildcards, Some(local_wildcard.clone()))
        }
        LocationUnion::Workspace(workspace_wildcard) => {
            let local_dirs = path_engine.workspace_dir_to_local_dir(&WorkspaceDir {
                workspace_name: workspace_wildcard.workspace_name.clone(),
                dirs: workspace_wildcard.dirs.clone(),
            });

            if let Some(local_dirs) = local_dirs {
                let local_wildcard = LocalPathWildcard {
                    dirs: local_dirs,
                    recursive: workspace_wildcard.recursive,
                    wildcard: workspace_wildcard.wildcard.clone(),
                };
                let depot_wildcards =
                    path_engine.local_wildcard_to_depot_candidate(&local_wildcard);
                (depot_wildcards, Some(local_wildcard))
            } else {
                (vec![], None)
            }
        }
        LocationUnion::Depot(depot_wildcard) => (vec![depot_wildcard.clone()], None),
    };

    // 用 depot_wildcard_candidates 对 depot_paths 进行过滤，并合成 Vec<FileLocation>
    let mut result = vec![];

    for wildcard in depot_wildcard_candidates {
        for path in depot_paths {
            if wildcard.match_and_get_diff(path).is_none() {
                continue;
            }
            let local_path = path_engine.mapping_depot_path(path);
            if local_path.is_none() {
                continue;
            }
            let local_path = local_path.unwrap();
            let workspace_path = path_engine.local_path_to_workspace_path(&local_path);
            if workspace_path.is_none() {
                continue;
            }
            let workspace_path = workspace_path.unwrap();
            // 如果 local_filter.is_some()，执行后置过滤操作
            if let Some(local_filter) = &local_filter {
                if local_filter.match_and_get_diff(&local_path).is_none() {
                    continue;
                }
            }

            result.push(FileLocation {
                local_path,
                workspace_path,
                depot_path: path.clone(),
            });
        }
    }

    return result;
}
