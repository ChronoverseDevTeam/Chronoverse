pub mod blob_controller;
pub mod user_controller;

use serde::{Deserialize, Serialize};

use crate::crv2::ChronoverseApp;

// ── Wire types ────────────────────────────────────────────────────────────────

/// Incoming RPC request (deserialized from JSON).
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum HiveRequest {
    RegisterUser { username: String, password: String },
    GetBlobTicket { hash: String },
}

/// Outgoing RPC response (serialized to JSON).
#[derive(Serialize)]
pub struct HiveResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HiveResponse {
    pub fn ok(data: serde_json::Value) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

/// Route a parsed [`HiveRequest`] to the appropriate controller.
pub async fn dispatch(app: &ChronoverseApp, req: HiveRequest) -> HiveResponse {
    match req {
        HiveRequest::RegisterUser { username, password } => {
            user_controller::register_user(app, username, password).await
        }
        HiveRequest::GetBlobTicket { hash } => {
            blob_controller::get_blob_ticket(app, hash).await
        }
    }
}