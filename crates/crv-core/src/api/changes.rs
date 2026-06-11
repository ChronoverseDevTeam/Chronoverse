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
use crate::engine::submit;
use crate::storage::depot::Depot;
use crate::AppState;

// ── List Changelists ───────────────────────────────────────────────

/// GET /api/v1/clients/:client/changes
pub async fn list_changes(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    let rows = sqlx::query(
        "SELECT id, number, description, status, created_at
         FROM changes WHERE client_id = $1
         ORDER BY COALESCE(number, 0) DESC, created_at DESC
         LIMIT 100",
    )
    .bind(client.id)
    .fetch_all(&state.db_pool)
    .await
    .map_err(internal_error)?;

    let changes: Vec<Value> = rows.iter().map(|r| json!({
        "id": r.get::<Uuid, _>("id"),
        "number": r.get::<Option<i64>, _>("number"),
        "description": r.get::<String, _>("description"),
        "status": r.get::<String, _>("status"),
        "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
    })).collect();

    Ok(Json(json!({"success": true, "data": changes})))
}

// ── Create Changelist ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateChangeRequest {
    pub description: String,
}

/// POST /api/v1/clients/:client/changes
pub async fn create_change(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(client_name): Path<String>,
    Json(req): Json<CreateChangeRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    if req.description.trim().is_empty() {
        return Err(bad_request("description is required"));
    }

    let row = sqlx::query(
        "INSERT INTO changes (client_id, user_id, description, status)
         VALUES ($1, $2, $3, 'pending')
         RETURNING id, number, description, status, created_at",
    )
    .bind(client.id)
    .bind(auth.user_id)
    .bind(req.description.trim())
    .fetch_one(&state.db_pool)
    .await
    .map_err(internal_error)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({"success": true, "data": {
            "id": row.get::<Uuid, _>("id"),
            "number": row.get::<Option<i64>, _>("number"),
            "description": row.get::<String, _>("description"),
            "status": row.get::<String, _>("status"),
            "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        }})),
    ))
}

// ── Get Changelist ─────────────────────────────────────────────────

/// GET /api/v1/clients/:client/changes/:id
pub async fn get_change(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path((client_name, change_id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    let row = sqlx::query(
        "SELECT id, number, description, status, created_at, user_id
         FROM changes WHERE id = $1 AND client_id = $2",
    )
    .bind(change_id)
    .bind(client.id)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(internal_error)?
    .ok_or_else(|| not_found("changelist not found"))?;

    Ok(Json(json!({"success": true, "data": {
        "id": row.get::<Uuid, _>("id"),
        "number": row.get::<Option<i64>, _>("number"),
        "description": row.get::<String, _>("description"),
        "status": row.get::<String, _>("status"),
        "user_id": row.get::<Uuid, _>("user_id"),
        "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
    }})))
}

// ── Submit Changelist ──────────────────────────────────────────────

/// POST /api/v1/clients/:client/changes/:id/submit
///
/// This triggers the atomic submit operation.
/// File content must be uploaded beforehand via POST /api/v1/files/:path/content.
pub async fn submit_change(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path((client_name, change_id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;
    let depot = Depot::new(&state.config.depot_root);

    // Ensure depot directory exists
    depot.init().await.map_err(internal_error)?;

    let result = submit::submit_changelist(
        &state.db_pool,
        &depot,
        change_id,
        client.id,
        auth.user_id,
    )
    .await
    .map_err(|e| match e {
        crv_shared::error::CrvError::NotFound(_) => not_found(&e.to_string()),
        crv_shared::error::CrvError::InvalidInput(_) => bad_request(&e.to_string()),
        _ => internal_error(e),
    })?;

    Ok(Json(json!({
        "success": true,
        "data": {
            "change_number": result.change_number,
            "files_submitted": result.files_submitted,
        }
    })))
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
