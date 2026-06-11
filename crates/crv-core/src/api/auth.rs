use axum::{extract::State, Json};
use crv_shared::types::LoginRequest;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{middleware::AuthUser, password, ticket};
use crate::AppState;

/// POST /api/v1/auth/login
///
/// Authenticates a user with username+password and returns a session ticket.
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    // Look up user by name
    let user_row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE name = $1"
    )
    .bind(&req.user)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error during login: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"success": false, "error": "internal server error"})),
        )
    })?;

    let (user_id, password_hash) = match user_row {
        Some(row) => row,
        None => {
            return Err((
                axum::http::StatusCode::UNAUTHORIZED,
                Json(json!({"success": false, "error": "invalid username or password"})),
            ));
        }
    };

    // Verify password
    let valid = password::verify_password(&req.password, &password_hash)
        .map_err(|e| {
            tracing::error!("Password verification error: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": "internal server error"})),
            )
        })?;

    if !valid {
        return Err((
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"success": false, "error": "invalid username or password"})),
        ));
    }

    // Generate ticket
    let now = chrono::Utc::now();
    let ticket_str = ticket::generate_ticket(
        state.config.ticket_secret.as_bytes(),
        user_id,
        now,
    )
    .map_err(|e| {
        tracing::error!("Ticket generation error: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"success": false, "error": "internal server error"})),
        )
    })?;

    // Store ticket in database
    ticket::store_ticket(
        &state.db_pool,
        user_id,
        &ticket_str,
        None,
        state.config.ticket_ttl_hours,
    )
    .await
    .map_err(|e| {
        tracing::error!("Ticket storage error: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"success": false, "error": "internal server error"})),
        )
    })?;

    tracing::info!("User '{}' logged in", req.user);

    Ok(Json(json!({
        "success": true,
        "data": {
            "ticket": ticket_str,
            "user": req.user,
        }
    })))
}

/// POST /api/v1/auth/logout
///
/// Invalidates the current session ticket.
pub async fn logout(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    // The ticket is extracted from the Authorization header in the middleware.
    // We need the raw ticket string to revoke it, but the middleware only gives us
    // the validated user_id. We revoke all tickets for this user.

    ticket::revoke_user_tickets(&state.db_pool, auth.user_id)
        .await
        .map_err(|e| {
            tracing::error!("Ticket revocation error: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": "internal server error"})),
            )
        })?;

    tracing::info!("User '{}' logged out", auth.user_name);

    Ok(Json(json!({
        "success": true,
        "data": {
            "message": "logged out successfully"
        }
    })))
}

/// GET /api/v1/auth/whoami
///
/// Returns the currently authenticated user's info.
pub async fn whoami(
    auth: AuthUser,
) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "user_id": auth.user_id.to_string(),
            "user_name": auth.user_name,
        }
    }))
}

/// POST /api/v1/bootstrap
///
/// One-time bootstrap: creates an admin user if no users exist yet.
/// Only works when the users table is empty.
pub async fn bootstrap(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    use crate::auth::password;

    // Check if any user exists
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db_pool)
        .await
        .map_err(|e| {
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{e}")})))
        })?;

    if count > 0 {
        return Err((
            axum::http::StatusCode::CONFLICT,
            Json(json!({"success": false, "error": "bootstrap already performed"})),
        ));
    }

    let admin_password = std::env::var("CRV_SMOKE_ADMIN_PASSWORD")
        .unwrap_or_else(|_| "admin123".into());

    let hash = password::hash_password(&admin_password).map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{e}")})))
    })?;

    sqlx::query(
        "INSERT INTO users (name, email, password_hash, user_type) VALUES ($1, $2, $3, 'operator')",
    )
    .bind("admin")
    .bind("admin@chronoverse.local")
    .bind(&hash)
    .execute(&state.db_pool)
    .await
    .map_err(|e| {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{e}")})))
    })?;

    tracing::info!("Bootstrap complete: admin user created");

    Ok(Json(json!({
        "success": true,
        "data": {"user": "admin", "message": "admin user created"}
    })))
}
