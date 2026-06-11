use axum::{
    extract::{Path, State},
    Json,
};
use crv_shared::types::{ClientSpec, CreateClientRequest, UpdateClientRequest};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::AppState;

// ── List ───────────────────────────────────────────────────────────

/// GET /api/v1/clients
pub async fn list_clients(
    State(state): State<Arc<AppState>>, _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query(
        "SELECT id, name, owner_id, root, view_json, options_json, host, stream_id, description, created_at, updated_at FROM clients ORDER BY name",
    )
    .fetch_all(&state.db_pool).await.map_err(internal_error)?;
    let clients: Vec<ClientSpec> = rows.iter().map(row_to_spec).collect();
    Ok(Json(json!({"success": true, "data": clients})))
}

// ── Get ────────────────────────────────────────────────────────────

/// GET /api/v1/clients/:name
pub async fn get_client(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query(
        "SELECT id, name, owner_id, root, view_json, options_json, host, stream_id, description, created_at, updated_at FROM clients WHERE name = $1",
    )
    .bind(&name).fetch_optional(&state.db_pool).await.map_err(internal_error)?
    .ok_or_else(|| not_found("client not found"))?;
    Ok(Json(json!({"success": true, "data": row_to_spec(&row)})))
}

// ── Create ─────────────────────────────────────────────────────────

/// POST /api/v1/clients
pub async fn create_client(
    State(state): State<Arc<AppState>>, auth: AuthUser, Json(req): Json<CreateClientRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    if req.name.trim().is_empty() { return Err(bad_request("client name is required")); }
    if req.root.trim().is_empty() { return Err(bad_request("client root is required")); }
    let view_json = serde_json::to_value(&req.view).map_err(internal_error)?;
    let options_json = serde_json::to_value(&req.options).map_err(internal_error)?;

    let row = sqlx::query(
        "INSERT INTO clients (name, owner_id, root, view_json, options_json, host, stream_id, description)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, name, owner_id, root, view_json, options_json, host, stream_id, description, created_at, updated_at",
    )
    .bind(req.name.trim()).bind(auth.user_id).bind(req.root.trim())
    .bind(&view_json).bind(&options_json)
    .bind(req.host.as_deref()).bind(req.stream_id).bind(req.description.as_deref())
    .fetch_one(&state.db_pool).await
    .map_err(|e| if e.to_string().contains("unique") { bad_request("client name already exists") } else { internal_error(e) })?;

    tracing::info!("Client '{}' created by '{}'", req.name, auth.user_name);
    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": row_to_spec(&row)}))))
}

// ── Update ─────────────────────────────────────────────────────────

/// PUT /api/v1/clients/:name
pub async fn update_client(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
    Json(req): Json<UpdateClientRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query("SELECT id FROM clients WHERE name = $1")
        .bind(&name).fetch_optional(&state.db_pool).await.map_err(internal_error)?
        .ok_or_else(|| not_found("client not found"))?;
    let cid: Uuid = row.get("id");

    if let Some(ref r) = req.root {
        sqlx::query("UPDATE clients SET root = $1, updated_at = now() WHERE id = $2")
            .bind(r).bind(cid).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref v) = req.view {
        let j = serde_json::to_value(v).map_err(internal_error)?;
        sqlx::query("UPDATE clients SET view_json = $1, updated_at = now() WHERE id = $2")
            .bind(&j).bind(cid).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref o) = req.options {
        let j = serde_json::to_value(o).map_err(internal_error)?;
        sqlx::query("UPDATE clients SET options_json = $1, updated_at = now() WHERE id = $2")
            .bind(&j).bind(cid).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref h) = req.host {
        sqlx::query("UPDATE clients SET host = $1, updated_at = now() WHERE id = $2")
            .bind(h).bind(cid).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref s) = req.stream_id {
        sqlx::query("UPDATE clients SET stream_id = $1, updated_at = now() WHERE id = $2")
            .bind(s).bind(cid).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref d) = req.description {
        sqlx::query("UPDATE clients SET description = $1, updated_at = now() WHERE id = $2")
            .bind(d).bind(cid).execute(&state.db_pool).await.map_err(internal_error)?;
    }

    let updated = sqlx::query(
        "SELECT id, name, owner_id, root, view_json, options_json, host, stream_id, description, created_at, updated_at FROM clients WHERE id = $1",
    )
    .bind(cid).fetch_one(&state.db_pool).await.map_err(internal_error)?;
    Ok(Json(json!({"success": true, "data": row_to_spec(&updated)})))
}

// ── Delete ─────────────────────────────────────────────────────────

/// DELETE /api/v1/clients/:name
pub async fn delete_client(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let r = sqlx::query("DELETE FROM clients WHERE name = $1")
        .bind(&name).execute(&state.db_pool).await.map_err(internal_error)?;
    if r.rows_affected() == 0 { return Err(not_found("client not found")); }
    tracing::info!("Client '{}' deleted", name);
    Ok(Json(json!({"success": true, "data": {"deleted": true}})))
}

// ── Row Mapping ────────────────────────────────────────────────────

fn row_to_spec(row: &sqlx::postgres::PgRow) -> ClientSpec {
    let v: serde_json::Value = row.get("view_json");
    let o: serde_json::Value = row.get("options_json");
    ClientSpec {
        id: row.get("id"), name: row.get("name"), owner_id: row.get("owner_id"),
        root: row.get("root"),
        view: serde_json::from_value(v).unwrap_or_default(),
        options: serde_json::from_value(o).unwrap_or_default(),
        host: row.get("host"), stream_id: row.get("stream_id"),
        description: row.get("description"),
        created_at: row.get("created_at"), updated_at: row.get("updated_at"),
    }
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
