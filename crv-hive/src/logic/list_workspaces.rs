use tonic::{Request, Response, Status};

use crate::pb::{ListWorkspaceReq, ListWorkspaceRsp, Workspace};

pub async fn list_workspaces(
    request: Request<ListWorkspaceReq>,
) -> Result<Response<ListWorkspaceRsp>, Status> {
    let req = request.into_inner();

    // 至少提供一个非空筛选条件，避免无约束全量列表
    let has_any_filter = req
        .name
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || req
            .owner
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        || req
            .device_finger_print
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
    if !has_any_filter {
        return Err(Status::invalid_argument("at least one filter required"));
    }

    let items = crate::database::workspace::list_workspaces_filtered(
        req.name.as_deref(),
        req.owner.as_deref(),
        req.device_finger_print.as_deref(),
    )
    .await
    .map_err(|_| Status::internal("db error"))?;

    let workspaces: Vec<Workspace> = items
        .into_iter()
        .map(|w| Workspace {
            name: w.name,
            created_at: w.created_at.timestamp(),
            updated_at: w.updated_at.timestamp(),
            owner: w.owner,
            path: w.path,
            device_finger_print: w.device_finger_print,
        })
        .collect();

    Ok(Response::new(ListWorkspaceRsp { workspaces }))
}
