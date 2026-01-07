use crate::auth::require_user;
use crate::hive_server::{derive_file_id_from_path, hive_dao};
use crate::pb::{LaunchSubmitReq, LaunchSubmitRsp};
use tonic::{Request, Response, Status};

pub async fn handle_launch_submit(
    request: Request<LaunchSubmitReq>,
) -> Result<Response<LaunchSubmitRsp>, Status> {
    Ok(Response::new(LaunchSubmitRsp {
        ticket: "".to_string(),
        success: true,
        file_unable_to_lock: Vec::new(),
    }))
}