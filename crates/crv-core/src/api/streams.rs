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
use crate::AppState;

// ── Helpers ────────────────────────────────────────────────────────

/// Valid stream types and which parent types they allow.
/// Mirrors Perforce rules: mainline has no parent; release/development
/// can inherit from mainline or same-type; task inherits from development.
fn validate_stream_type(stream_type: &str, parent_type: Option<&str>) -> Result<(), String> {
    match stream_type {
        "mainline" => match parent_type {
            Some(_) => Err("mainline stream cannot have a parent".into()),
            None => Ok(()),
        },
        "release" => match parent_type {
            Some("mainline") | None => Ok(()),
            Some(t) => Err(format!("release stream parent must be mainline, not {t}")),
        },
        "development" => match parent_type {
            Some("mainline") | Some("development") | None => Ok(()),
            Some(t) => Err(format!("development stream parent must be mainline or development, not {t}")),
        },
        "task" => match parent_type {
            Some("development") | None => Ok(()),
            Some(t) => Err(format!("task stream parent must be development, not {t}")),
        },
        "virtual" => Ok(()), // virtual can have any or no parent
        _ => Err(format!("unknown stream type: {stream_type}")),
    }
}

/// Resolve a parent stream by name, returning (id, stream_type).
async fn resolve_parent(
    pool: &sqlx::PgPool,
    parent_name: Option<&str>,
) -> Result<(Option<Uuid>, Option<String>), (axum::http::StatusCode, Json<Value>)> {
    let Some(name) = parent_name else { return Ok((None, None)) };
    let name = name.trim();
    if name.is_empty() { return Ok((None, None)) }

    let row = sqlx::query("SELECT id, stream_type FROM streams WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found(&format!("parent stream '{name}' not found")))?;

    Ok((Some(row.get("id")), Some(row.get("stream_type"))))
}

/// Merge parent views into child views (child overrides parent).
/// Each view entry: {"depot_path": "...", "path_type": "share"|"isolate"|"import"|"exclude"}
fn merge_views(parent_views: &[Value], child_views: &[Value]) -> Vec<Value> {
    let mut result: Vec<Value> = parent_views.to_vec();
    for cv in child_views {
        let cp = cv["depot_path"].as_str().unwrap_or("");
        // Remove parent entry for the same depot_path if child overrides it
        result.retain(|pv| pv["depot_path"].as_str() != Some(cp));
        result.push(cv.clone());
    }
    result
}

// ── List Streams ───────────────────────────────────────────────────

/// GET /api/v1/streams
pub async fn list_streams(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query(
        "SELECT id, name, parent_id, stream_type, description, created_at FROM streams ORDER BY name",
    )
    .fetch_all(&state.db_pool)
    .await
    .map_err(internal_error)?;

    let streams: Vec<Value> = rows.iter().map(|r| json!({
        "id": r.get::<Uuid, _>("id"),
        "name": r.get::<String, _>("name"),
        "parent_id": r.get::<Option<Uuid>, _>("parent_id"),
        "stream_type": r.get::<String, _>("stream_type"),
        "description": r.get::<Option<String>, _>("description"),
        "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
    })).collect();

    Ok(Json(json!({"success": true, "data": streams})))
}

// ── Create Stream ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateStreamRequest {
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub parent_name: Option<String>,
    #[serde(default = "default_stream_type")]
    pub stream_type: String,
    pub description: Option<String>,
}

fn default_stream_type() -> String { "development".into() }

/// POST /api/v1/streams
pub async fn create_stream(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(req): Json<CreateStreamRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    let name = req.name.trim();
    if name.is_empty() { return Err(bad_request("stream name is required")); }

    // Resolve parent: prefer parent_id, then parent_name
    let (parent_id, parent_type) = if let Some(pid) = req.parent_id {
        // Look up the type of the already-known parent
        let row = sqlx::query("SELECT stream_type FROM streams WHERE id = $1")
            .bind(pid)
            .fetch_optional(&state.db_pool)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| not_found(&format!("parent stream '{pid}' not found")))?;
        (Some(pid), Some(row.get::<String, _>("stream_type")))
    } else if let Some(ref pn) = req.parent_name {
        resolve_parent(&state.db_pool, Some(pn)).await?
    } else {
        (None, None)
    };

    // Validate stream type against parent type
    if let Err(msg) = validate_stream_type(&req.stream_type, parent_type.as_deref()) {
        return Err(bad_request(&msg));
    }

    let row = sqlx::query(
        "INSERT INTO streams (name, parent_id, stream_type, owner_id, description)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, parent_id, stream_type, description, created_at",
    )
    .bind(name)
    .bind(parent_id)
    .bind(&req.stream_type)
    .bind(auth.user_id)
    .bind(req.description.as_deref())
    .fetch_one(&state.db_pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") { bad_request("stream already exists") }
        else { internal_error(e) }
    })?;

    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": {
        "id": row.get::<Uuid, _>("id"),
        "name": row.get::<String, _>("name"),
        "parent_id": row.get::<Option<Uuid>, _>("parent_id"),
        "stream_type": row.get::<String, _>("stream_type"),
        "description": row.get::<Option<String>, _>("description"),
    }}))))
}

// ── Get Single Stream ──────────────────────────────────────────────

/// GET /api/v1/streams/:name
pub async fn get_stream(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query(
        "SELECT id, name, parent_id, stream_type, view_json, options_json, description, created_at, updated_at
         FROM streams WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(internal_error)?
    .ok_or_else(|| not_found("stream not found"))?;

    let parent_name: Option<String> = if let Some(pid) = row.get::<Option<Uuid>, _>("parent_id") {
        sqlx::query_scalar("SELECT name FROM streams WHERE id = $1")
            .bind(pid)
            .fetch_optional(&state.db_pool)
            .await
            .map_err(internal_error)?
    } else { None };

    Ok(Json(json!({"success": true, "data": {
        "id": row.get::<Uuid, _>("id"),
        "name": row.get::<String, _>("name"),
        "parent_id": row.get::<Option<Uuid>, _>("parent_id"),
        "parent_name": parent_name,
        "stream_type": row.get::<String, _>("stream_type"),
        "views": row.get::<serde_json::Value, _>("view_json"),
        "options": row.get::<serde_json::Value, _>("options_json"),
        "description": row.get::<Option<String>, _>("description"),
        "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        "updated_at": row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
    }})))
}

// ── Set Stream Views ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetViewsRequest {
    pub views: Vec<Value>,
}

/// PUT /api/v1/streams/:name/views
pub async fn set_stream_views(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
    Json(req): Json<SetViewsRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let r = sqlx::query(
        "UPDATE streams SET view_json = $1, updated_at = now() WHERE name = $2",
    )
    .bind(serde_json::to_value(&req.views).unwrap_or_default())
    .bind(&name)
    .execute(&state.db_pool)
    .await
    .map_err(internal_error)?;

    if r.rows_affected() == 0 { return Err(not_found("stream not found")); }

    Ok(Json(json!({"success": true, "data": {"views": req.views}})))
}

/// GET /api/v1/streams/:name/views  — returns effective (inherited) views
pub async fn get_stream_views(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    // Get stream + all ancestors bottom-up
    let row = sqlx::query(
        "SELECT id, parent_id, view_json FROM streams WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(internal_error)?
    .ok_or_else(|| not_found("stream not found"))?;

    let mut views: Vec<Value> = serde_json::from_value(
        row.get::<serde_json::Value, _>("view_json"),
    ).unwrap_or_default();

    // Walk up parent chain, merging views (parent first, child overrides)
    let mut current_pid: Option<Uuid> = row.get("parent_id");
    while let Some(pid) = current_pid {
        let parent_row = sqlx::query(
            "SELECT parent_id, view_json FROM streams WHERE id = $1",
        )
        .bind(pid)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?;

        if let Some(pr) = parent_row {
            let parent_views: Vec<Value> = serde_json::from_value(
                pr.get::<serde_json::Value, _>("view_json"),
            ).unwrap_or_default();
            views = merge_views(&parent_views, &views);
            current_pid = pr.get("parent_id");
        } else {
            break;
        }
    }

    Ok(Json(json!({"success": true, "data": {"stream": name, "effective_views": views}})))
}

// ── Delete Stream ──────────────────────────────────────────────────

/// DELETE /api/v1/streams/:name
pub async fn delete_stream(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let r = sqlx::query("DELETE FROM streams WHERE name = $1")
        .bind(&name)
        .execute(&state.db_pool)
        .await
        .map_err(internal_error)?;
    if r.rows_affected() == 0 { return Err(not_found("stream not found")); }
    Ok(Json(json!({"success": true})))
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
