use tonic::{Request, Response, Status};

use crate::pb::{ListWorkspaceReq, ListWorkspaceRsp};

pub async fn list_workspaces(
    request: Request<ListWorkspaceReq>,
) -> Result<Response<ListWorkspaceRsp>, Status> {
    Ok(Response::new(ListWorkspaceRsp { workspaces: vec![] }))
}