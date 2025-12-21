use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crate::pb::{BonjourReq, BonjourRsp};
use tonic::{Request, Response};

pub async fn handle(
    state: AppState,
    _req: Request<BonjourReq>,
) -> AppResult<Response<BonjourRsp>> {
    let hive_client = state.get_hive_client().await?;
    let mut client = hive_client.lock().await;
    
    let hive_rsp = client.bonjour().await
        .map_err(|e| AppError::HiveClient(e.to_string()))?;
    
    let response = BonjourRsp {
        daemon_version: format!("{}.{}", hive_rsp.major_version, hive_rsp.minor_version),
        api_level: 1,
        platform: hive_rsp.platform,
        os: hive_rsp.os,
        architecture: hive_rsp.architecture,
    };

    println!("Hive bonjour RSP: {:?}", response);
    
    Ok(Response::new(response))
}

