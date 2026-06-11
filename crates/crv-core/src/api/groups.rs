use axum::{
    extract::{Path, State},
    Json,
};
use crv_shared::types::{CreateGroupRequest, Group, ModifyGroupMembersRequest, UpdateGroupRequest};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::AppState;

// ── List ───────────────────────────────────────────────────────────

/// GET /api/v1/groups
pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query("SELECT id, name, description, created_at FROM groups ORDER BY name")
        .fetch_all(&state.db_pool).await.map_err(internal_error)?;
    let mut result: Vec<Group> = Vec::new();
    for row in &rows {
        let gid: Uuid = row.get("id");
        result.push(Group {
            id: gid, name: row.get("name"), description: row.get("description"),
            members: fetch_members(&state.db_pool, gid).await?,
            created_at: row.get("created_at"),
        });
    }
    Ok(Json(json!({"success": true, "data": result})))
}

// ── Get ────────────────────────────────────────────────────────────

/// GET /api/v1/groups/:name
pub async fn get_group(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query("SELECT id, name, description, created_at FROM groups WHERE name = $1")
        .bind(&name).fetch_optional(&state.db_pool).await.map_err(internal_error)?
        .ok_or_else(|| not_found("group not found"))?;
    let gid: Uuid = row.get("id");
    Ok(Json(json!({"success": true, "data": Group {
        id: gid, name: row.get("name"), description: row.get("description"),
        members: fetch_members(&state.db_pool, gid).await?,
        created_at: row.get("created_at"),
    }})))
}

// ── Create ─────────────────────────────────────────────────────────

/// POST /api/v1/groups
pub async fn create_group(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Json(req): Json<CreateGroupRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    if req.name.trim().is_empty() { return Err(bad_request("group name is required")); }
    let row = sqlx::query(
        "INSERT INTO groups (name, description) VALUES ($1, $2) RETURNING id, name, description, created_at",
    )
    .bind(req.name.trim()).bind(req.description.as_deref())
    .fetch_one(&state.db_pool).await
    .map_err(|e| if e.to_string().contains("unique") { bad_request("group name already exists") } else { internal_error(e) })?;
    let gid: Uuid = row.get("id");
    for uid in &req.members {
        let _ = sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(gid).bind(uid).execute(&state.db_pool).await;
    }
    let group = Group {
        id: gid, name: row.get("name"), description: row.get("description"),
        members: fetch_members(&state.db_pool, gid).await?,
        created_at: row.get("created_at"),
    };
    tracing::info!("Group '{}' created", group.name);
    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": group}))))
}

// ── Update ─────────────────────────────────────────────────────────

/// PUT /api/v1/groups/:name
pub async fn update_group(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
    Json(req): Json<UpdateGroupRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query("SELECT id, name, description, created_at FROM groups WHERE name = $1")
        .bind(&name).fetch_optional(&state.db_pool).await.map_err(internal_error)?
        .ok_or_else(|| not_found("group not found"))?;
    let gid: Uuid = row.get("id");
    if let Some(ref desc) = req.description {
        sqlx::query("UPDATE groups SET description = $1 WHERE id = $2")
            .bind(desc).bind(gid).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    let updated = sqlx::query("SELECT id, name, description, created_at FROM groups WHERE id = $1")
        .bind(gid).fetch_one(&state.db_pool).await.map_err(internal_error)?;
    Ok(Json(json!({"success": true, "data": Group {
        id: gid, name: updated.get("name"), description: updated.get("description"),
        members: fetch_members(&state.db_pool, gid).await?,
        created_at: updated.get("created_at"),
    }})))
}

// ── Delete ─────────────────────────────────────────────────────────

/// DELETE /api/v1/groups/:name
pub async fn delete_group(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let r = sqlx::query("DELETE FROM groups WHERE name = $1")
        .bind(&name).execute(&state.db_pool).await.map_err(internal_error)?;
    if r.rows_affected() == 0 { return Err(not_found("group not found")); }
    tracing::info!("Group '{}' deleted", name);
    Ok(Json(json!({"success": true, "data": {"deleted": true}})))
}

// ── Members ────────────────────────────────────────────────────────

/// POST /api/v1/groups/:name/members
pub async fn modify_group_members(
    State(state): State<Arc<AppState>>, _auth: AuthUser, Path(name): Path<String>,
    Json(req): Json<ModifyGroupMembersRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query("SELECT id, name, description, created_at FROM groups WHERE name = $1")
        .bind(&name).fetch_optional(&state.db_pool).await.map_err(internal_error)?
        .ok_or_else(|| not_found("group not found"))?;
    let gid: Uuid = row.get("id");
    for uid in &req.add {
        let _ = sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(gid).bind(uid).execute(&state.db_pool).await;
    }
    for uid in &req.remove {
        let _ = sqlx::query("DELETE FROM group_members WHERE group_id = $1 AND user_id = $2")
            .bind(gid).bind(uid).execute(&state.db_pool).await;
    }
    Ok(Json(json!({"success": true, "data": Group {
        id: gid, name: row.get("name"), description: row.get("description"),
        members: fetch_members(&state.db_pool, gid).await?,
        created_at: row.get("created_at"),
    }})))
}

// ── Helpers ────────────────────────────────────────────────────────

async fn fetch_members(pool: &sqlx::PgPool, gid: Uuid) -> Result<Vec<Uuid>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query("SELECT user_id FROM group_members WHERE group_id = $1")
        .bind(gid).fetch_all(pool).await.map_err(internal_error)?;
    Ok(rows.iter().map(|r| r.get("user_id")).collect())
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
