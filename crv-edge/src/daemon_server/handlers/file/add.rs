use crate::daemon_server::db::active_file::Action;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{expand_to_mapped_files_in_fs, normalize_path};
use crate::daemon_server::state::AppState;
use crate::pb::{AddReq, AddRsp};
use crv_core::path::engine::PathEngine;
use tonic::{Request, Response, Status};

pub async fn handle(state: AppState, req: Request<AddReq>) -> AppResult<Response<AddRsp>> {
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

    let mut added_paths = Vec::new();

    for path in &request_body.paths {
        let location_union = normalize_path(&path, &path_engine)?;

        let local_files = expand_to_mapped_files_in_fs(&location_union, &path_engine);

        let guard = state.db.prepare_command(&local_files, &[])?;

        for file in &guard.paths {
            // 设置为 active file，action 为 Add
            state.db.set_active_file_action(file.clone(), Action::Add)?;
            added_paths.push(file.to_custom_string());
        }
    }

    Ok(Response::new(AddRsp { added_paths }))
}
