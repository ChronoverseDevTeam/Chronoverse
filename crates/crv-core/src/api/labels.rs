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
use crate::engine::label;
use crate::AppState;

// ── Label Sync ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LabelSyncRequest {
    pub filespec: String,
}

/// POST /api/v1/labels/:name/sync
///
/// Tag the current head revisions of matching files to this label.
pub async fn sync_label(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
    Json(req): Json<LabelSyncRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let label_id = resolve_label(&state, &name).await?;
    let count = label::sync_label(&state.db_pool, label_id, &req.filespec)
        .await
        .map_err(|e| match &e {
            crv_shared::error::CrvError::NotFound(_) => not_found(&e.to_string()),
            _ => internal_error(e),
        })?;

    Ok(Json(json!({"success": true, "data": {"files_tagged": count}})))
}

/// GET /api/v1/labels/:name/revisions
///
/// List revisions tagged by this label.
pub async fn list_label_revisions(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let label_id = resolve_label(&state, &name).await?;
    let revs = label::list_label_revisions(&state.db_pool, label_id)
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({"success": true, "data": revs})))
}

/// POST /api/v1/labels/:name/clear
///
/// Remove all revision tags from a label.
pub async fn clear_label(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let label_id = resolve_label(&state, &name).await?;
    let count = label::clear_label(&state.db_pool, label_id)
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({"success": true, "data": {"removed": count}})))
}

// ── Label CRUD ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateLabelRequest {
    pub name: String,
    pub description: Option<String>,
}

/// GET /api/v1/labels
pub async fn list_labels(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query("SELECT id, name, description, created_at FROM labels ORDER BY name")
        .fetch_all(&state.db_pool)
        .await
        .map_err(internal_error)?;

    let labels: Vec<Value> = rows.iter().map(|r| json!({
        "id": r.get::<Uuid, _>("id"),
        "name": r.get::<String, _>("name"),
        "description": r.get::<Option<String>, _>("description"),
        "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
    })).collect();

    Ok(Json(json!({"success": true, "data": labels})))
}

/// POST /api/v1/labels
pub async fn create_label(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateLabelRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    if req.name.trim().is_empty() { return Err(bad_request("label name is required")); }

    let row = sqlx::query(
        "INSERT INTO labels (name, owner_id, description) VALUES ($1, $2, $3)
         RETURNING id, name, description, created_at",
    )
    .bind(req.name.trim())
    .bind(auth.user_id)
    .bind(req.description.as_deref())
    .fetch_one(&state.db_pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") { bad_request("label already exists") }
        else { internal_error(e) }
    })?;

    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": {
        "id": row.get::<Uuid, _>("id"),
        "name": row.get::<String, _>("name"),
        "description": row.get::<Option<String>, _>("description"),
    }}))))
}

/// DELETE /api/v1/labels/:name
pub async fn delete_label(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let r = sqlx::query("DELETE FROM labels WHERE name = $1")
        .bind(&name)
        .execute(&state.db_pool)
        .await
        .map_err(internal_error)?;
    if r.rows_affected() == 0 { return Err(not_found("label not found")); }
    Ok(Json(json!({"success": true, "data": {"deleted": true}})))
}

// ── Helpers ────────────────────────────────────────────────────────

async fn resolve_label(
    state: &AppState,
    name: &str,
) -> Result<Uuid, (axum::http::StatusCode, Json<Value>)> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM labels WHERE name = $1")
        .bind(name)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("label not found"))
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
