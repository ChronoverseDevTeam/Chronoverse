use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::engine::lock;
use crate::AppState;

// ── Lock ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LockRequest {
    pub files: Vec<String>,
}

/// POST /api/v1/files/lock
pub async fn lock_files(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(client_name): Path<String>,
    Json(req): Json<LockRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    for path in &req.files {
        lock::lock_file(&state.db_pool, path, client.id, auth.user_id)
            .await
            .map_err(|e| match e {
                crv_shared::error::CrvError::FileLocked { .. } => bad_request(&e.to_string()),
                _ => internal_error(e),
            })?;
    }

    Ok(Json(json!({"success": true, "data": {"locked": req.files.len()}})))
}

/// POST /api/v1/files/unlock
pub async fn unlock_files(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<LockRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let mut count = 0;
    for path in &req.files {
        if lock::unlock_file(&state.db_pool, path, auth.user_id)
            .await
            .map_err(|e| match e {
                crv_shared::error::CrvError::InvalidInput(_) => bad_request(&e.to_string()),
                _ => internal_error(e),
            })?
        {
            count += 1;
        }
    }
    Ok(Json(json!({"success": true, "data": {"unlocked": count}})))
}

/// GET /api/v1/files/locks
pub async fn list_locks(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let locks = lock::list_locks(&state.db_pool, None)
        .await
        .map_err(internal_error)?;

    let data: Vec<Value> = locks.iter().map(|l| json!({
        "depot_path": l.depot_path,
        "user_id": l.user_id,
        "user_name": l.user_name,
        "lock_type": l.lock_type,
        "created_at": l.created_at,
    })).collect();

    Ok(Json(json!({"success": true, "data": data})))
}

// ── Helpers ────────────────────────────────────────────────────────

async fn resolve_client(
    state: &AppState,
    name: &str,
) -> Result<ClientInfo, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query("SELECT id FROM clients WHERE name = $1")
        .bind(name)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("client not found"))?;
    Ok(ClientInfo { id: row.get("id") })
}

struct ClientInfo {
    id: Uuid,
}

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
