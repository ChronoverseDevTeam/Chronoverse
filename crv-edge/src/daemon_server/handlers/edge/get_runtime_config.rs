use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{GetRuntimeConfigReq, GetRuntimeConfigRsp, RuntimeConfigItem};
use tonic::{Request, Response};

pub async fn handle(
    _state: AppState,
    req: Request<GetRuntimeConfigReq>,
) -> AppResult<Response<GetRuntimeConfigRsp>> {
    let runtime_config = RuntimeConfig::from_req(&req)?;

    let response = GetRuntimeConfigRsp {
        remote_addr: Some(runtime_config.remote_addr.into()),
        editor: Some(runtime_config.editor.into()),
        user: Some(runtime_config.user.into()),
    };

    Ok(Response::new(response))
}
