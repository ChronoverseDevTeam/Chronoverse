use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{BonjourReq, BonjourRsp};
use crv_core::{log_debug, log_info};
use tonic::{Request, Response};

pub async fn handle(
    _state: AppState,
    _req: Request<BonjourReq>,
) -> AppResult<Response<BonjourRsp>> {
    log_debug!("edge::bonjour handler invoked");
    let response = BonjourRsp {
        daemon_version: "1.0.0-local-test".to_string(),
        api_level: 1,
        platform: "chronoverse".to_string(),
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
    };

    log_info!(
        daemon_version = %response.daemon_version,
        os = %response.os,
        architecture = %response.architecture,
        "edge::bonjour ok"
    );
    Ok(Response::new(response))
}
