use serde_json::json;

use crate::crv2::{ChronoverseApp, RegisterUserReq};

use super::HiveResponse;

/// Handle `register_user` RPC.
pub async fn register_user(
    app: &ChronoverseApp,
    username: String,
    password: String,
) -> HiveResponse {
    match app
        .register_user(&RegisterUserReq { username, password })
        .await
    {
        Ok(rsp) => HiveResponse::ok(json!({"username": rsp.username})),
        Err(e) => HiveResponse::err(e),
    }
}
