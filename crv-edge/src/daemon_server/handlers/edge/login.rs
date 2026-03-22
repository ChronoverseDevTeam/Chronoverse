use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crate::hive_pb::{self, hive_service_client::HiveServiceClient};
use crate::pb::{LoginReq, LoginRsp};
use tonic::{Request, Response, Status};

pub async fn handle(state: AppState, req: Request<LoginReq>) -> AppResult<Response<LoginRsp>> {
    const UNAUTH_USER: &str = "default";

    let runtime_config = RuntimeConfig::from_req(&req)?;
    let request = req.into_inner();

    if request.username.trim().is_empty() || request.password.is_empty() {
        return Err(AppError::from(Status::invalid_argument(
            "username and password are required",
        )));
    }

    let channel = state
        .hive_channel
        .get_channel(&runtime_config.remote_addr.value)?;

    let mut hive_client = HiveServiceClient::new(channel);
    let hive_rsp = hive_client
        .login(hive_pb::LoginReq {
            username: request.username.clone(),
            password: request.password,
        })
        .await?
        .into_inner();

    // If we are switching accounts, clear the old per-user token to avoid stale tokens lingering
    // on disk. "default" is treated as the unauthenticated placeholder user.
    let old_user = state.db.load_runtime_config()?.user;
    if let Some(old_user) = old_user {
        let old_user = old_user.trim().to_string();
        if !old_user.is_empty() && old_user != UNAUTH_USER && old_user != request.username {
            let old_per_user_key = format!("auth-token:{}", old_user);
            state.db.set_config(&old_per_user_key, "")?;
        }
    }

    state.db.set_config("user", &request.username)?;
    let per_user_key = format!("auth-token:{}", request.username);
    state.db.set_config(&per_user_key, &hive_rsp.access_token)?;

    Ok(Response::new(LoginRsp {
        expires_at: hive_rsp.expires_at,
    }))
}
