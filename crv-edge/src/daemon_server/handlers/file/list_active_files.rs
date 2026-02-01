use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{expand_to_mapped_files_in_fs, normalize_paths_strict};
use crate::daemon_server::state::AppState;
use crate::pb::{ActiveFileInfo, ListActiveFilesReq, ListActiveFilesRsp};
use crv_core::path::engine::PathEngine;
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

    let path_engine = PathEngine::new(workspace_meta.config.clone(), &request_body.workspace_name);

    // 2. 规范化路径，获取目录路径
    let local_paths = normalize_paths_strict(&[request_body.path], &path_engine)?;

    if local_paths.is_empty() {
        return Ok(Response::new(ListActiveFilesRsp {
            active_files: vec![],
        }));
    }

    // 3. 展开为文件列表
    let local_files = expand_to_mapped_files_in_fs(&local_paths, &path_engine);

    // 4. 获取所有 active files
    let mut active_files = vec![];
    for file in local_files {
        let action = state.db.get_active_file_action(&file.workspace_path)?;
        if let Some(action) = action {
            active_files.push(ActiveFileInfo {
                path: file.workspace_path.to_custom_string(),
                action: action.to_custom_string(),
            })
        }
    }

    Ok(Response::new(ListActiveFilesRsp { active_files }))
}
