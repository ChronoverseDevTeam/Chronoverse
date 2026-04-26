use serde_json::json;

use crate::crv2::ChronoverseApp;

use super::HiveResponse;

/// Handle `register_user` RPC.
pub async fn register_user(
    app: &ChronoverseApp,
    username: String,
    password: String,
) -> HiveResponse {
    match app.register_user(&username, &password).await {
        Ok(user) => HiveResponse::ok(json!({"username": user.username})),
        Err(e) => HiveResponse::err(e),
    }
}
