use tonic::{Request, Response, Status};

use crate::pb::{ListWorkspaceReq, ListWorkspaceRsp};

pub async fn list_workspaces(
    request: Request<ListWorkspaceReq>,
) -> Result<Response<ListWorkspaceRsp>, Status> {
    let _req = request.into_inner();
    Ok(Response::new(ListWorkspaceRsp { workspaces: vec![] }))
}