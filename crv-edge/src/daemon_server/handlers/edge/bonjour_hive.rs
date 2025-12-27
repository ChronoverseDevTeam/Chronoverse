use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::error::AppResult;
use crate::daemon_server::state::AppState;
use crate::hive_pb::{self, hive_service_client::HiveServiceClient};
use crate::pb::{BonjourReq, BonjourRsp};
use tonic::{Request, Response};

pub async fn handle(state: AppState, req: Request<BonjourReq>) -> AppResult<Response<BonjourRsp>> {
    let runtime_config = RuntimeConfig::from_req(&req)?;
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

    println!("Hive bonjour RSP: {:?}", response);

    Ok(Response::new(response))
}
