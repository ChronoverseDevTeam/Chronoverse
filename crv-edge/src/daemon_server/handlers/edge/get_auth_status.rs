use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{GetAuthStatusReq, GetAuthStatusRsp};
use tonic::{Request, Response};

pub async fn handle(
    state: AppState,
    _req: Request<GetAuthStatusReq>,
) -> AppResult<Response<GetAuthStatusRsp>> {
    let runtime = state.db.load_runtime_config()?;
    let current_user = runtime.user.unwrap_or_else(|| "default".to_string());
    let logged_in = runtime
        .auth_token
        .map(|token| !token.is_empty())
        .unwrap_or(false);

    Ok(Response::new(GetAuthStatusRsp {
        current_user,
        logged_in,
    }))
}
