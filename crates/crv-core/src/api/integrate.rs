use axum::{
    extract::{State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::engine::integrate;
use crate::storage::depot::Depot;
use crate::AppState;

// ── Integrate ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IntegrateRequest {
    pub source: String,
    pub target: String,
    #[serde(default = "default_action")]
    pub action: String,
    pub change_id: Option<Uuid>,
}

fn default_action() -> String { "branch_from".into() }

/// POST /api/v1/integrate
pub async fn do_integrate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<IntegrateRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let depot = Depot::new(&state.config.depot_root);
    depot.init().await.map_err(internal_error)?;

    let valid_actions = ["branch_from", "merge_from", "copy_from"];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(bad_request(&format!(
            "invalid action '{}'; must be one of: {}",
            req.action,
            valid_actions.join(", ")
        )));
    }

    let result = integrate::integrate_files(
        &state.db_pool,
        &depot,
        &req.source,
        &req.target,
        auth.user_id,
        req.change_id,
        &req.action,
    )
    .await
    .map_err(|e| match &e {
        crv_shared::error::CrvError::NotFound(_) => not_found(&e.to_string()),
        _ => internal_error(e),
    })?;

    Ok(Json(json!({
        "success": true,
        "data": {
            "files_branched": result.files_branched,
            "integration_records": result.integration_records,
        }
    })))
}

/// GET /api/v1/integrations?path=//depot/...
#[derive(Deserialize)]
pub struct IntegrationsQuery {
    pub path: Option<String>,
}

pub async fn list_integrations_handler(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    axum::extract::Query(q): axum::extract::Query<IntegrationsQuery>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let records = integrate::list_integrations(&state.db_pool, q.path.as_deref())
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({"success": true, "data": records})))
}

// ── Error helpers ──────────────────────────────────────────────────

fn internal_error<E: std::fmt::Display>(e: E) -> (axum::http::StatusCode, Json<Value>) {
    tracing::error!("{e}");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"success": false, "error": format!("{e}")})))
}
fn not_found(msg: &str) -> (axum::http::StatusCode, Json<Value>) {
    (axum::http::StatusCode::NOT_FOUND, Json(json!({"success": false, "error": msg})))
}
fn bad_request(msg: &str) -> (axum::http::StatusCode, Json<Value>) {
    (axum::http::StatusCode::BAD_REQUEST, Json(json!({"success": false, "error": msg})))
}
