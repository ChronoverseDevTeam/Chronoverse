use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{expand_paths_to_files, normalize_paths};
use crate::daemon_server::state::AppState;
use crate::daemon_server::db::active_file::Action;
use crate::pb::{AddReq, AddRsp};
use crv_core::path::basic::LocalPath;
use crv_core::path::engine::PathEngine;
use tonic::{Request, Response, Status};

pub async fn handle(
    state: AppState,
    req: Request<AddReq>,
) -> AppResult<Response<AddRsp>> {
    let request_body = req.into_inner();
    
    // 1. 获取 workspace 信息
    let workspace_meta = state
        .db
        .get_confirmed_workspace_meta(&request_body.workspace_name)?
        .ok_or(AppError::Raw(Status::not_found(format!(
            "Workspace {} not found.",
            request_body.workspace_name
        ))))?;

    // 2. 规范化路径
    let local_paths = normalize_paths(
        &request_body.paths,
        &request_body.workspace_name,
        &workspace_meta.config,
    )?;

    // 3. 展开为文件列表
    let local_files = expand_paths_to_files(&local_paths);
    // 4. 转换为 workspace paths 并标记为 Add
    let path_engine = PathEngine::new(workspace_meta.config.clone(), &request_body.workspace_name);
    let mut added_paths = Vec::new();

    for file in &local_files {
        let local_path = LocalPath::parse(file)
            .map_err(|e| AppError::Raw(Status::invalid_argument(format!("Invalid path: {}", e))))?;
        
        if let Some(workspace_path) = path_engine.local_path_to_workspace_path(&local_path) {
            // 设置为 active file，action 为 Add
            state.db.set_active_file_action(workspace_path.clone(), Action::Add)?;
            added_paths.push(workspace_path.to_string());
        }
    }

    Ok(Response::new(AddRsp { added_paths }))
}

