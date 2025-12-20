use std::path::{self, Path, PathBuf};

use crate::daemon_server::context::SessionContext;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crate::pb::{CreateWorkspaceReq, CreateWorkspaceRsp};
use crv_core::workspace::entity::WorkspaceConfig;
use tonic::{Request, Response, Status};

pub async fn handle(
    state: AppState,
    req: Request<CreateWorkspaceReq>,
) -> AppResult<Response<CreateWorkspaceRsp>> {
    let ctx = SessionContext::from_req(&req)?;
    let req = req.into_inner();

    // step 0. 检查 root dir 是否存在，且为目录
    let root_dir = PathBuf::from(&req.workspace_root);
    if !root_dir.is_dir() {
        return Err(AppError::from(Status::invalid_argument(format!(
            "Root dir {} does not exists or is file.",
            &req.workspace_root
        ))));
    }
    if !root_dir.is_absolute() {
        return Err(AppError::from(Status::invalid_argument(format!(
            "Root dir {} is not absolute path.",
            &req.workspace_root
        ))));
    }
    let mut root_dir = root_dir.to_string_lossy().to_string();
    // 对目录进行归一化，保证能够被 LocalDir 的 parser 解析
    if !root_dir.ends_with("/") && !root_dir.ends_with("\\") {
        root_dir = format!("{}{}", root_dir, path::MAIN_SEPARATOR)
    }

    // Step 1: 创建 WorkspaceConfig 结构，验证用户输入的 mapping 是否合法
    let workspace_config =
        WorkspaceConfig::from_specification(&req.workspace_name, &root_dir, &req.workspace_mapping)
            .map_err(|e| {
                Status::invalid_argument(format!("Invalid workspace configuration: {}", e))
            })?;

    // Step 2: 调用 create_workspace_pending 创建 Pending 状态的 workspace
    state
        .db
        .create_workspace_pending(req.workspace_name.clone(), workspace_config.clone())
        .map_err(|e| Status::internal(format!("Failed to create pending workspace: {}", e)))?;

    // Step 3: 调用 hive 的接口注册这个 workspace（暂时留空）
    // TODO: 实现 hive 注册逻辑
    // let hive_client = state.get_hive_client().await?;
    // let register_req = RegisterWorkspaceReq {
    //     workspace_name: req.workspace_name.clone(),
    //     workspace_root: req.workspace_root.clone(),
    //     workspace_mapping: req.workspace_mapping.clone(),
    // };
    // hive_client.register_workspace(register_req).await.map_err(|e| {
    //     // 如果 hive 注册失败，需要回滚本地数据库中的 pending workspace
    //     // TODO: 实现回滚逻辑
    //     Status::internal(format!("Failed to register workspace with hive: {}", e))
    // })?;

    // Step 4: 调用 confirm_workspace 将 workspace 元数据状态改为 Confirmed
    state
        .db
        .confirm_workspace(req.workspace_name.clone())
        .map_err(|e| Status::internal(format!("Failed to confirm workspace: {}", e)))?;

    // 返回成功响应
    Ok(Response::new(CreateWorkspaceRsp {}))
}
