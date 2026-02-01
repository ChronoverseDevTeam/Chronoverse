use crate::daemon_server::db::active_file::Action;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{
    expand_to_mapped_files_in_edge_meta, normalize_paths_strict,
};
use crate::daemon_server::state::AppState;
use crate::pb::{DeleteReq, DeleteRsp};
use crv_core::path::engine::PathEngine;
use tonic::{Request, Response, Status};

pub async fn handle(state: AppState, req: Request<DeleteReq>) -> AppResult<Response<DeleteRsp>> {
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

    // 2. 规范化路径
    let local_paths = normalize_paths_strict(&request_body.paths, &path_engine)?;

    // 3. 展开为文件列表
    let files = expand_to_mapped_files_in_edge_meta(&local_paths, &path_engine, state.clone())?;

    // 4. 转换为 workspace paths 并标记为 Delete
    let mut deleted_paths = Vec::new();

    for file in &files {
        // 设置为 active file，action 为 Delete
        state
            .db
            .set_active_file_action(file.workspace_path.clone(), Action::Delete)?;
        deleted_paths.push(file.workspace_path.to_custom_string());
    }

    Ok(Response::new(DeleteRsp { deleted_paths }))
}
