pub mod blob_controller;
pub mod depot_controller;
pub mod pre_submit_controller;
pub mod submit_controller;
pub mod user_controller;

use serde::{Deserialize, Serialize};

use crate::crv2::ChronoverseApp;
use pre_submit_controller::PreSubmitFilePayload;

// ── Wire types ────────────────────────────────────────────────────────────────

/// Incoming RPC request (deserialized from JSON).
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum HiveRequest {
    RegisterUser { username: String, password: String },
    GetBlobTicket { hash: String },
    BrowseDepotTree { path: String, changelist_id: i64, #[serde(default)] recursive: bool },
    QueryPathHistory {
        path: String,
        from_changelist: Option<i64>,
        to_changelist: Option<i64>,
        limit: Option<usize>,
    },
    PreSubmit { description: String, files: Vec<PreSubmitFilePayload> },
    Submit { submit_id: i64 },
    CancelSubmit { submit_id: i64 },
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
        HiveRequest::BrowseDepotTree { path, changelist_id, recursive } => {
            depot_controller::browse_depot_tree(app, path, changelist_id, recursive).await
        }
        HiveRequest::QueryPathHistory {
            path,
            from_changelist,
            to_changelist,
            limit,
        } => {
            depot_controller::query_path_history(app, path, from_changelist, to_changelist, limit).await
        }
        HiveRequest::PreSubmit { description, files } => {
            pre_submit_controller::pre_submit(app, description, files).await
        }
        HiveRequest::Submit { submit_id } => {
            submit_controller::submit(app, submit_id).await
        }
        HiveRequest::CancelSubmit { submit_id } => {
            submit_controller::cancel_submit(app, submit_id).await
        }
    }
}