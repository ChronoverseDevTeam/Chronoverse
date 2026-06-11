use axum::{
    extract::{Path, State},
    Json,
};
use crv_shared::types::{CreateUserRequest, UpdateUserRequest, User, UserType};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{middleware::AuthUser, password};
use crate::AppState;

// ── List Users ─────────────────────────────────────────────────────

/// GET /api/v1/users
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query(
        "SELECT id, name, email, full_name, user_type, created_at, updated_at FROM users ORDER BY name",
    )
    .fetch_all(&state.db_pool)
    .await
    .map_err(internal_error)?;

    let users: Vec<User> = rows.iter().map(row_to_user).collect();
    Ok(Json(json!({"success": true, "data": users})))
}

// ── Get User ───────────────────────────────────────────────────────

/// GET /api/v1/users/:id
pub async fn get_user(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let row = sqlx::query(
        "SELECT id, name, email, full_name, user_type, created_at, updated_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(internal_error)?;

    match row {
        Some(r) => Ok(Json(json!({"success": true, "data": row_to_user(&r)}))),
        None => Err(not_found("user not found")),
    }
}

// ── Create User ────────────────────────────────────────────────────

/// POST /api/v1/users
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Json(req): Json<CreateUserRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    if req.name.trim().is_empty() {
        return Err(bad_request("user name is required"));
    }
    if req.email.trim().is_empty() {
        return Err(bad_request("email is required"));
    }
    if req.password.len() < 6 {
        return Err(bad_request("password must be at least 6 characters"));
    }

    let password_hash = password::hash_password(&req.password).map_err(internal_error)?;
    let user_type_str = user_type_to_str(&req.user_type);

    let row = sqlx::query(
        "INSERT INTO users (name, email, full_name, password_hash, user_type)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, email, full_name, user_type, created_at, updated_at",
    )
    .bind(req.name.trim())
    .bind(req.email.trim())
    .bind(req.full_name.as_deref())
    .bind(&password_hash)
    .bind(user_type_str)
    .fetch_one(&state.db_pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            bad_request("user name or email already exists")
        } else {
            internal_error(e)
        }
    })?;

    let user = row_to_user(&row);
    tracing::info!("User '{}' created", user.name);
    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": user}))))
}

// ── Update User ────────────────────────────────────────────────────

/// PUT /api/v1/users/:id
pub async fn update_user(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(user_id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let existing = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("user not found"))?;

    if let Some(ref email) = req.email {
        sqlx::query("UPDATE users SET email = $1, updated_at = now() WHERE id = $2")
            .bind(email)
            .bind(existing)
            .execute(&state.db_pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("unique") { bad_request("email already in use") }
                else { internal_error(e) }
            })?;
    }
    if let Some(ref full_name) = req.full_name {
        sqlx::query("UPDATE users SET full_name = $1, updated_at = now() WHERE id = $2")
            .bind(full_name).bind(existing).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref pwd) = req.password {
        if pwd.len() < 6 { return Err(bad_request("password must be at least 6 characters")); }
        let hash = password::hash_password(pwd).map_err(internal_error)?;
        sqlx::query("UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2")
            .bind(&hash).bind(existing).execute(&state.db_pool).await.map_err(internal_error)?;
    }
    if let Some(ref ut) = req.user_type {
        sqlx::query("UPDATE users SET user_type = $1, updated_at = now() WHERE id = $2")
            .bind(user_type_to_str(ut)).bind(existing).execute(&state.db_pool).await.map_err(internal_error)?;
    }

    let row = sqlx::query(
        "SELECT id, name, email, full_name, user_type, created_at, updated_at FROM users WHERE id = $1",
    )
    .bind(existing)
    .fetch_one(&state.db_pool)
    .await
    .map_err(internal_error)?;

    Ok(Json(json!({"success": true, "data": row_to_user(&row)})))
}

// ── Delete User ────────────────────────────────────────────────────

/// DELETE /api/v1/users/:id
pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id).execute(&state.db_pool).await.map_err(internal_error)?;
    if result.rows_affected() == 0 { return Err(not_found("user not found")); }
    tracing::info!("User '{}' deleted", user_id);
    Ok(Json(json!({"success": true, "data": {"deleted": true}})))
}

// ── Row Mapping ────────────────────────────────────────────────────

fn row_to_user(row: &sqlx::postgres::PgRow) -> User {
    let ut_str: String = row.get("user_type");
    let user_type = match ut_str.as_str() {
        "operator" => UserType::Operator,
        "service" => UserType::Service,
        _ => UserType::Standard,
    };
    User {
        id: row.get("id"),
        name: row.get("name"),
        email: row.get("email"),
        full_name: row.get("full_name"),
        user_type,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn user_type_to_str(ut: &UserType) -> &'static str {
    match ut {
        UserType::Standard => "standard",
        UserType::Operator => "operator",
        UserType::Service => "service",
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
