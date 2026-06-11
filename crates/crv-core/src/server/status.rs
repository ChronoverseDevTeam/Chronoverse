use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;

/// Health check — returns database connectivity status.
pub async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.db_pool)
        .await
        .is_ok();

    Json(json!({
        "success": true,
        "data": {
            "status": if db_ok { "ok" } else { "degraded" },
            "version": env!("CARGO_PKG_VERSION"),
            "db_connected": db_ok,
        }
    }))
}

/// Server info — returns configuration (non-sensitive).
pub async fn info(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "version": env!("CARGO_PKG_VERSION"),
            "depot_root": state.config.depot_root,
            "ticket_ttl_hours": state.config.ticket_ttl_hours,
        }
    }))
}
