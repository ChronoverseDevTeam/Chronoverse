use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::hive_pb::{self, hive_service_client::HiveServiceClient};
use crate::pb::{BonjourReq, BonjourRsp};
use crv_core::{log_debug, log_info};
use tonic::{Request, Response};

pub async fn handle(state: AppState, req: Request<BonjourReq>) -> AppResult<Response<BonjourRsp>> {
    let runtime_config = RuntimeConfig::from_req(&req)?;
    log_debug!(remote_addr = %runtime_config.remote_addr.value, "edge::bonjour_hive handler invoked");
    let channel = state
        .hive_channel
        .get_channel(&runtime_config.remote_addr.value)?;

    let mut hive_client = HiveServiceClient::new(channel.clone());

    let req = hive_pb::BonjourReq {};
    let hive_rsp = hive_client.bonjour(req).await?.into_inner();

    let response = BonjourRsp {
        daemon_version: format!("{}.{}", hive_rsp.major_version, hive_rsp.minor_version),
        api_level: 1,
        platform: hive_rsp.platform,
        os: hive_rsp.os,
        architecture: hive_rsp.architecture,
    };

    log_info!(
        daemon_version = %response.daemon_version,
        os = %response.os,
        architecture = %response.architecture,
        "edge::bonjour_hive ok"
    );
    Ok(Response::new(response))
}
