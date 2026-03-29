use crate::daemon_server::db::active_file::Action;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{expand_to_mapped_files_in_fs, normalize_path};
use crate::daemon_server::state::AppState;
use crate::pb::{CheckoutReq, CheckoutRsp};
use crv_core::path::engine::PathEngine;
use std::collections::HashSet;
use tonic::{Request, Response, Status};

pub async fn handle(
    state: AppState,
    req: Request<CheckoutReq>,
) -> AppResult<Response<CheckoutRsp>> {
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

    // 规范化路径，展开为文件列表
    let mut files = vec![];
    for path in &request_body.paths {
        let location_union = normalize_path(path, &path_engine)?;
        files.extend(expand_to_mapped_files_in_fs(&location_union, &path_engine));
    }

    // 由于 request_body.paths 可能有重叠的路径范围，这里还要做一次去重
    let mut seen = HashSet::new();
    files.retain(|x| seen.insert(x.workspace_path.to_custom_string()));

    let _file_guard = state.db.prepare_command(&files)?;

    // 4. 转换为 workspace paths 并标记为 Edit
    let mut checkout_paths = Vec::new();

    for file in &files {
        // 如果无法获取文件 meta，则说明文件不存在于远端或尚未拉新，跳过此文件
        if state.db.get_file_meta(&file.workspace_path)?.is_none() {
            continue;
        }

        // 如果文件已经存在于 active file，则跳过此文件
        if state
            .db
            .get_active_file_action(&file.workspace_path)?
            .is_some()
        {
            continue;
        }

        // 设置为 active file 的 action 为 Edit
        state
            .db
            .set_active_file_action(file.workspace_path.clone(), Action::Edit)?;
        checkout_paths.push(file.workspace_path.to_custom_string());
    }

    Ok(Response::new(CheckoutRsp {
        checkouted_paths: checkout_paths,
    }))
}
