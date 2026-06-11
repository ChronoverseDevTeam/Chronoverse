use axum::{extract::DefaultBodyLimit, routing::{delete, get, post}, Json, Router};
use std::sync::Arc;

mod status;

use crate::api;
use crate::AppState;

/// Build the full Axum application router.
pub fn build_router(state: Arc<AppState>) -> Router {
    // Auth routes
    let auth_routes = Router::new()
        .route("/login", post(api::auth::login))
        .route("/logout", post(api::auth::logout))
        .route("/whoami", get(api::auth::whoami));

    // User routes
    let user_routes = Router::new()
        .route("/", get(api::users::list_users).post(api::users::create_user))
        .route("/{id}", get(api::users::get_user).put(api::users::update_user).delete(api::users::delete_user));

    // Group routes
    let group_routes = Router::new()
        .route("/", get(api::groups::list_groups).post(api::groups::create_group))
        .route("/{name}", get(api::groups::get_group).put(api::groups::update_group).delete(api::groups::delete_group))
        .route("/{name}/members", post(api::groups::modify_group_members));

    // Client routes
    let client_routes = Router::new()
        .route("/", get(api::clients::list_clients).post(api::clients::create_client))
        .route("/{name}", get(api::clients::get_client).put(api::clients::update_client).delete(api::clients::delete_client));

    // File routes (per-client operations)
    let client_file_routes = Router::new()
        .route("/add", post(api::files::open_for_add))
        .route("/edit", post(api::files::open_for_edit))
        .route("/delete", post(api::files::open_for_delete))
        .route("/revert", post(api::files::revert_files))
        .route("/opened", get(api::files::list_opened));

    // Per-client routes
    let per_client = Router::new()
        .nest("/files", client_file_routes)
        .route("/files/lock", post(api::locks::lock_files))
        .route("/sync", get(api::files::sync_client))
        .route("/sync/confirm", post(api::files::confirm_sync))
        .route("/changes", get(api::changes::list_changes).post(api::changes::create_change))
        .route("/changes/{id}", get(api::changes::get_change))
        .route("/changes/{id}/submit", post(api::changes::submit_change));

    // Global file routes (content, fstat, filelog)
    let file_routes = Router::new()
        .route("/content/{*depot_path}", get(api::files::download_file).post(api::files::upload_file))
        .route("/fstat/{*depot_path}", get(api::files::file_stat))
        .route("/filelog/{*depot_path}", get(api::files::file_log))
        .route("/unlock", post(api::locks::unlock_files))
        .route("/locks", get(api::locks::list_locks));

    // Label routes
    let label_routes = Router::new()
        .route("/", get(api::labels::list_labels).post(api::labels::create_label))
        .route("/{name}", delete(api::labels::delete_label))
        .route("/{name}/sync", post(api::labels::sync_label))
        .route("/{name}/revisions", get(api::labels::list_label_revisions))
        .route("/{name}/clear", post(api::labels::clear_label));

    // Protection routes
    let protection_routes = Router::new()
        .route("/", get(api::protections::list_protections).post(api::protections::add_protection))
        .route("/{id}", delete(api::protections::delete_protection));

    // Stream routes
    let stream_routes = Router::new()
        .route("/", get(api::streams::list_streams).post(api::streams::create_stream))
        .route("/{name}", get(api::streams::get_stream).delete(api::streams::delete_stream))
        .route("/{name}/views", get(api::streams::get_stream_views).put(api::streams::set_stream_views));

    // Branch routes
    let branch_routes = Router::new()
        .route("/", get(list_branches_handler).post(create_branch_handler))
        .route("/{name}", delete(delete_branch_handler));

    let api_v1 = Router::new()
        .nest("/auth", auth_routes)
        .route("/bootstrap", post(api::auth::bootstrap))
        .nest("/users", user_routes)
        .nest("/groups", group_routes)
        .nest("/clients", client_routes)
        .nest("/clients/{client}", per_client)
        .nest("/files", file_routes)
        .nest("/labels", label_routes)
        .nest("/protections", protection_routes)
        .nest("/streams", stream_routes)
        .nest("/branches", branch_routes)
        .route("/integrate", post(api::integrate::do_integrate))
        .route("/integrations", get(api::integrate::list_integrations_handler))
        .route("/health", get(status::health))
        .route("/info", get(status::info));

    Router::new()
        .nest("/api/v1", api_v1)
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024)) // 100 MB for large file uploads
        .with_state(state)
}

// ── Branch Handlers (inline for simplicity) ─────────────────────────

use axum::extract::{Path, State as AxumState};
use serde_json::{json, Value};
use uuid::Uuid;
use sqlx::Row;

async fn list_branches_handler(
    AxumState(state): AxumState<Arc<AppState>>,
    _auth: crate::auth::middleware::AuthUser,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let rows = sqlx::query("SELECT id, name, description, created_at FROM branches ORDER BY name")
        .fetch_all(&state.db_pool).await.map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{e}")}))))?;
    let branches: Vec<Value> = rows.iter().map(|r| json!({
        "id": r.get::<Uuid, _>("id"), "name": r.get::<String, _>("name"),
        "description": r.get::<Option<String>, _>("description"),
    })).collect();
    Ok(Json(json!({"success": true, "data": branches})))
}

async fn create_branch_handler(
    AxumState(state): AxumState<Arc<AppState>>,
    auth: crate::auth::middleware::AuthUser,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), (axum::http::StatusCode, Json<Value>)> {
    let name = body["name"].as_str().unwrap_or("");
    if name.is_empty() { return Err((axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "name required"})))); }
    let row = sqlx::query("INSERT INTO branches (name, owner_id, description) VALUES ($1, $2, $3) RETURNING id, name")
        .bind(name).bind(auth.user_id).bind(body["description"].as_str())
        .fetch_one(&state.db_pool).await.map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{e}")}))))?;
    Ok((axum::http::StatusCode::CREATED, Json(json!({"success": true, "data": {"id": row.get::<Uuid, _>("id"), "name": row.get::<String, _>("name")}}))))
}

async fn delete_branch_handler(
    AxumState(state): AxumState<Arc<AppState>>,
    _auth: crate::auth::middleware::AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    sqlx::query("DELETE FROM branches WHERE name = $1").bind(&name).execute(&state.db_pool).await.map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{e}")}))))?;
    Ok(Json(json!({"success": true})))
}
