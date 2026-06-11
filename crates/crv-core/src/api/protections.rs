use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::engine::protect;
use crate::AppState;

// ── List Protections ───────────────────────────────────────────────

/// GET /api/v1/protections
pub async fn list_protections(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = protect::list_protections(&state.db_pool)
        .await
        .map_err(internal_error)?;
    Ok(Json(json!({"success": true, "data": rows})))
}

// ── Add Protection ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddProtectionRequest {
    pub perm_type: String,
    pub perm_level: String,
    pub entity_type: String,
    pub entity_name: String,
    pub depot_path_pattern: String,
    #[serde(default)]
    pub order: i32,
}

/// POST /api/v1/protections
pub async fn add_protection(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(req): Json<AddProtectionRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    let id = protect::add_protection(
        &state.db_pool,
        &req.perm_type,
        &req.perm_level,
        &req.entity_type,
        &req.entity_name,
        &req.depot_path_pattern,
        req.order,
    )
    .await
    .map_err(internal_error)?;

    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": {"id": id}}))))
}

/// DELETE /api/v1/protections/:id
pub async fn delete_protection(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let deleted = protect::delete_protection(&state.db_pool, id)
        .await
        .map_err(internal_error)?;
    if deleted {
        Ok(Json(json!({"success": true})))
    } else {
        Err(not_found("protection entry not found"))
    }
}

fn internal_error<E: std::fmt::Display>(e: E) -> (axum::http::StatusCode, Json<Value>) {
    tracing::error!("{e}");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"success": false, "error": format!("{e}")})))
}
fn not_found(msg: &str) -> (axum::http::StatusCode, Json<Value>) {
    (axum::http::StatusCode::NOT_FOUND, Json(json!({"success": false, "error": msg})))
}
