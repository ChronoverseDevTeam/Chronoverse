use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use crv_shared::error::CrvError;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;

/// Extracted authenticated user context, available to handlers.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub user_name: String,
}

/// Extracts auth from the `Authorization: Ticket <token>` header.
///
/// Usage in handlers: `async fn handler(auth: AuthUser) -> impl IntoResponse { ... }`
impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthRejection::MissingTicket)?;

        let ticket = auth_header
            .strip_prefix("Ticket ")
            .ok_or(AuthRejection::InvalidFormat)?;

        let user_id = crate::auth::ticket::validate_ticket(
            &state.db_pool,
            state.config.ticket_secret.as_bytes(),
            ticket,
        )
        .await
        .map_err(|e| match e {
            CrvError::AuthFailed(_) => AuthRejection::InvalidTicket(e.to_string()),
            _ => AuthRejection::InternalError,
        })?;

        let user_name: String = sqlx::query_scalar(
            "SELECT name FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&state.db_pool)
        .await
        .map_err(|_| AuthRejection::InternalError)?;

        Ok(AuthUser { user_id, user_name })
    }
}

/// Optional auth — succeeds without a ticket, returning `None`.
#[derive(Debug, Clone)]
pub struct OptionalAuth {
    pub user: Option<AuthUser>,
}

impl FromRequestParts<Arc<AppState>> for OptionalAuth {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalAuth { user: Some(user) }),
            Err(_) => Ok(OptionalAuth { user: None }),
        }
    }
}

// ── Rejection ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AuthRejection {
    MissingTicket,
    InvalidFormat,
    InvalidTicket(String),
    InternalError,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthRejection::MissingTicket => (
                StatusCode::UNAUTHORIZED,
                "Authentication required. Use 'crv login' first.",
            ),
            AuthRejection::InvalidFormat => (
                StatusCode::UNAUTHORIZED,
                "Invalid Authorization header. Expected 'Ticket <token>'.",
            ),
            AuthRejection::InvalidTicket(ref msg) => {
                // Note: we can't return a borrow, so we format inline
                let m = format!("Invalid or expired ticket: {msg}");
                // Use status + Json approach instead
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"success": false, "error": m})),
                )
                    .into_response();
            }
            AuthRejection::InternalError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Authentication service error.",
            ),
        };

        (status, Json(json!({"success": false, "error": message}))).into_response()
    }
}
