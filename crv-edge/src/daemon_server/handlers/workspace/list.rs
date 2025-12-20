use crate::daemon_server::context::SessionContext;
use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{ListWorkspacesReq, ListWorkspacesRsp};
use tonic::{Request, Response, Status};

pub async fn handle(
    state: AppState,
    req: Request<ListWorkspacesReq>,
) -> AppResult<Response<ListWorkspacesRsp>> {
    let _ctx = SessionContext::from_req(&req)?;

    // 从数据库获取所有 workspace
    let workspace_names = state
        .db
        .get_all_workspaces()
        .map_err(|e| Status::internal(format!("Failed to list workspaces: {}", e)))?;

    // 返回响应
    Ok(Response::new(ListWorkspacesRsp { workspace_names }))
}
