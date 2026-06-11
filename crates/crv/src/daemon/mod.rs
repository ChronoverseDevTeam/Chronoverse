/// REST daemon mode — local server that proxies to crv-core
/// and maintains local workspace state (SQLite db.have).

use axum::{
    extract::State,
    http::{Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::workspace::LocalWorkspace;

mod proxy;
use proxy::proxy_request;

/// Daemon application state.
pub struct DaemonState {
    pub core_url: String,
    pub ticket: Option<String>,
    pub workspace: Arc<Mutex<LocalWorkspace>>,
    pub http: Client,
}

impl DaemonState {
    pub fn new(core_url: String, ticket: Option<String>, workspace: LocalWorkspace) -> Self {
        Self { core_url, ticket, workspace: Arc::new(Mutex::new(workspace)), http: Client::new() }
    }
}

/// Build the daemon router.
pub fn build_router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/api/v1/daemon/status", get(daemon_status))
        .route("/api/v1/daemon/workspace", get(daemon_workspace))
        .fallback(proxy_handler)
        .with_state(state)
}

async fn daemon_status(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let ws = state.workspace.lock().await;
    Json(json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "core_url": state.core_url,
        "workspace_root": ws.root.to_string_lossy(),
    }))
}

async fn daemon_workspace(State(state): State<Arc<DaemonState>>) -> Json<Value> {
    let ws = state.workspace.lock().await;
    let count = ws.count_have();
    Json(json!({"root": ws.root.to_string_lossy(), "files_synced": count}))
}

/// Catch-all proxy: forward unrecognized requests to crv-core.
async fn proxy_handler(
    State(state): State<Arc<DaemonState>>,
    method: Method,
    uri: Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let target = format!("{}{}", state.core_url.trim_end_matches('/'), path);

    match proxy_request(&state.http, &target, &method, &headers, &body, state.ticket.as_deref()).await {
        Ok(response) => response,
        Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({"error": format!("proxy error: {e}")}))).into_response(),
    }
}
