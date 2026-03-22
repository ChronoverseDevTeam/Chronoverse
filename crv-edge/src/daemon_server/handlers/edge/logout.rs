use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{LogoutReq, LogoutRsp};
use tonic::{Request, Response};

pub async fn handle(state: AppState, _req: Request<LogoutReq>) -> AppResult<Response<LogoutRsp>> {
    const UNAUTH_USER: &str = "default";

    if let Some(user) = state.db.load_runtime_config()?.user {
        let user = user.trim().to_string();
        if !user.is_empty() && user != UNAUTH_USER {
            let per_user_key = format!("auth-token:{}", user);
            state.db.set_config(&per_user_key, "")?;
        }
    }
    // Clear legacy token key for backward compatibility.
    state.db.set_config("auth-token", "")?;
    state.db.set_config("user", "default")?;

    Ok(Response::new(LogoutRsp {}))
}
