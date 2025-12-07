use crate::daemon_server::context::SessionContext;
use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{CreateWorkspaceReq, CreateWorkspaceRsp};
use tonic::{Request, Response};

async fn handle(
    state: AppState,
    req: Request<CreateWorkspaceReq>,
) -> AppResult<Response<CreateWorkspaceRsp>> {
    let ctx = SessionContext::from_req(&req)?;
    todo!()
}
