use crate::daemon_server::db::active_file::Action;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::normalize_paths;
use crate::daemon_server::state::AppState;
use crate::pb::{ActiveFileInfo, ListActiveFilesReq, ListActiveFilesRsp};
use crv_core::path::basic::{LocalPath, WorkspaceDir};
use tonic::{Request, Response, Status};

pub async fn handle(
    state: AppState,
    req: Request<ListActiveFilesReq>,
) -> AppResult<Response<ListActiveFilesRsp>> {
    let request_body = req.into_inner();

    // 1. 获取 workspace 信息
    let workspace_meta = state
        .db
        .get_confirmed_workspace_meta(&request_body.workspace_name)?
        .ok_or(AppError::Raw(Status::not_found(format!(
            "Workspace {} not found.",
            request_body.workspace_name
        ))))?;

    // 2. 规范化路径，获取目录路径
    let local_paths = normalize_paths(
        &[request_body.path],
        &request_body.workspace_name,
        &workspace_meta.config,
    )?;

    if local_paths.is_empty() {
        return Ok(Response::new(ListActiveFilesRsp {
            active_files: vec![],
        }));
    }

    // 3. 解析为 LocalPath 并获取目录
    let local_path_str = &local_paths[0];
    let workspace_dir = if local_path_str.ends_with('/') || local_path_str.ends_with('\\') {
        // 如果是目录路径
        let local_dir = crv_core::path::basic::LocalDir::parse(local_path_str)
            .map_err(|e| AppError::Raw(Status::invalid_argument(format!("Invalid path: {}", e))))?;
        local_dir.into_workspace_dir(&request_body.workspace_name, &workspace_meta.config.root_dir)
    } else {
        // 如果是文件路径，取其父目录
        let local_path = LocalPath::parse(local_path_str)
            .map_err(|e| AppError::Raw(Status::invalid_argument(format!("Invalid path: {}", e))))?;
        WorkspaceDir {
            workspace_name: request_body.workspace_name.clone(),
            dirs: local_path.into_workspace_path(&request_body.workspace_name, &workspace_meta.config.root_dir).dirs,
        }
    };

    // 4. 获取该目录下的所有 active files
    let active_files_data = state.db.get_active_file_under_dir(&workspace_dir)?;

    // 5. 转换为响应格式
    let active_files = active_files_data
        .into_iter()
        .map(|(workspace_path, action)| ActiveFileInfo {
            path: workspace_path.to_string(),
            action: match action {
                Action::Add => "add".to_string(),
                Action::Edit => "edit".to_string(),
                Action::Delete => "delete".to_string(),
            },
        })
        .collect();

    Ok(Response::new(ListActiveFilesRsp { active_files }))
}

