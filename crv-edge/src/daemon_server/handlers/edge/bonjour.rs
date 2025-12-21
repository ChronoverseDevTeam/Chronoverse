use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::pb::{BonjourReq, BonjourRsp};
use tonic::{Request, Response};

pub async fn handle(
    _state: AppState,
    _req: Request<BonjourReq>,
) -> AppResult<Response<BonjourRsp>> {
    let response = BonjourRsp {
        daemon_version: "1.0.0-local-test".to_string(),
        api_level: 1,
        platform: "chronoverse".to_string(),
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
    };

    println!("Edge bonjour RSP: {:?}", response);
    Ok(Response::new(response))
}
