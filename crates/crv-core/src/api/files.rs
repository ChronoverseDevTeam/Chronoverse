use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::engine::sync;
use crate::storage::depot::Depot;
use crate::AppState;
use crv_shared::types::FileActionType;

/// Normalize a depot path extracted from a URL to the canonical form
/// (with `//` prefix). Axum may collapse multiple slashes in the URL.
fn normalize_depot_path(p: &str) -> String {
    let trimmed = p.trim_start_matches('/');
    if trimmed.is_empty() {
        return "//depot".to_string();
    }
    format!("//{trimmed}")
}

// ── Open for Add ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FileOpenRequest {
    pub files: Vec<String>,
    #[serde(default)]
    pub change_id: Option<Uuid>,
    pub file_type: Option<String>,
}

/// POST /api/v1/clients/:client/files/add
pub async fn open_for_add(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
    Json(req): Json<FileOpenRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;
    let change_id = resolve_change(&state, client.id, req.change_id).await?;

    for path in &req.files {
        if !crv_shared::proto::validate_depot_path(path) {
            return Err(bad_request(&format!("invalid depot path: {path}")));
        }
        sqlx::query(
            "INSERT INTO working (client_id, depot_path, action, change_id)
             VALUES ($1, $2, 'add', $3)
             ON CONFLICT (client_id, depot_path)
             DO UPDATE SET action = 'add', change_id = $3",
        )
        .bind(client.id)
        .bind(path)
        .bind(change_id)
        .execute(&state.db_pool)
        .await
        .map_err(internal_error)?;
    }

    Ok(Json(json!({"success": true, "data": {"opened": req.files.len()}})))
}

/// POST /api/v1/clients/:client/files/edit
pub async fn open_for_edit(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
    Json(req): Json<FileOpenRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;
    let change_id = resolve_change(&state, client.id, req.change_id).await?;

    for path in &req.files {
        // Verify file exists in depot
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM file_revisions WHERE depot_path = $1",
        )
        .bind(path)
        .fetch_one(&state.db_pool)
        .await
        .map_err(internal_error)?;

        if exists == 0 {
            return Err(bad_request(&format!("file not in depot: {path}")));
        }

        // Get base revision
        let base_rev: Option<i32> = sqlx::query_scalar(
            "SELECT MAX(revision) FROM file_revisions WHERE depot_path = $1",
        )
        .bind(path)
        .fetch_one(&state.db_pool)
        .await
        .map_err(internal_error)?;

        sqlx::query(
            "INSERT INTO working (client_id, depot_path, action, change_id, base_revision)
             VALUES ($1, $2, 'edit', $3, $4)
             ON CONFLICT (client_id, depot_path)
             DO UPDATE SET action = 'edit', change_id = $3, base_revision = $4",
        )
        .bind(client.id)
        .bind(path)
        .bind(change_id)
        .bind(base_rev)
        .execute(&state.db_pool)
        .await
        .map_err(internal_error)?;
    }

    Ok(Json(json!({"success": true, "data": {"opened": req.files.len()}})))
}

/// POST /api/v1/clients/:client/files/delete
pub async fn open_for_delete(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
    Json(req): Json<FileOpenRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;
    let change_id = resolve_change(&state, client.id, req.change_id).await?;

    for path in &req.files {
        sqlx::query(
            "INSERT INTO working (client_id, depot_path, action, change_id)
             VALUES ($1, $2, 'delete', $3)
             ON CONFLICT (client_id, depot_path)
             DO UPDATE SET action = 'delete', change_id = $3",
        )
        .bind(client.id)
        .bind(path)
        .bind(change_id)
        .execute(&state.db_pool)
        .await
        .map_err(internal_error)?;
    }

    Ok(Json(json!({"success": true, "data": {"opened": req.files.len()}})))
}

/// POST /api/v1/clients/:client/files/revert
pub async fn revert_files(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
    Json(req): Json<FileOpenRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    if req.files.is_empty() {
        sqlx::query("DELETE FROM working WHERE client_id = $1")
            .bind(client.id)
            .execute(&state.db_pool)
            .await
            .map_err(internal_error)?;
    } else {
        for path in &req.files {
            sqlx::query("DELETE FROM working WHERE client_id = $1 AND depot_path = $2")
                .bind(client.id)
                .bind(path)
                .execute(&state.db_pool)
                .await
                .map_err(internal_error)?;
        }
    }

    Ok(Json(json!({"success": true, "data": {"reverted": true}})))
}

/// GET /api/v1/clients/:client/files/opened
pub async fn list_opened(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    let rows = sqlx::query(
        "SELECT depot_path, action, change_id, base_revision FROM working WHERE client_id = $1 ORDER BY depot_path",
    )
    .bind(client.id)
    .fetch_all(&state.db_pool)
    .await
    .map_err(internal_error)?;

    let files: Vec<Value> = rows.iter().map(|r| {
        json!({
            "depot_path": r.get::<String, _>("depot_path"),
            "action": r.get::<String, _>("action"),
            "change_id": r.get::<Option<Uuid>, _>("change_id"),
            "base_revision": r.get::<Option<i32>, _>("base_revision"),
        })
    }).collect();

    Ok(Json(json!({"success": true, "data": files})))
}

// ── File Content ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ContentQuery {
    pub rev: Option<i32>,
}

/// GET /api/v1/files/:depot_path/content?rev=N
///
/// Download file content. Streams directly from blob storage for any file size.
pub async fn download_file(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(depot_path): Path<String>,
    Query(q): Query<ContentQuery>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let depot_path = normalize_depot_path(&depot_path);
    let rev = q.rev.unwrap_or(-1); // -1 means latest

    let row = if rev == -1 {
        sqlx::query(
            "SELECT digest, file_type FROM file_revisions
             WHERE depot_path = $1
             ORDER BY revision DESC LIMIT 1",
        )
        .bind(&depot_path)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?
    } else {
        sqlx::query(
            "SELECT digest, file_type FROM file_revisions WHERE depot_path = $1 AND revision = $2",
        )
        .bind(&depot_path)
        .bind(rev)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?
    }
    .ok_or_else(|| not_found("file revision not found"))?;

    let digest: String = row.get("digest");
    let depot = Depot::new(&state.config.depot_root);

    // Stream blob from disk — no in-memory buffering
    let file = depot.read_blob_stream(&digest).await.map_err(internal_error)?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

    let mime = if row.get::<String, _>("file_type") == "text" {
        "text/plain"
    } else {
        "application/octet-stream"
    };

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", depot_path.rsplit('/').next().unwrap_or("file")))
        .body(body)
        .unwrap())
}

/// POST /api/v1/files/:depot_path/content
///
/// Upload file content before submit. Streams content to disk for
/// files of any size (no in-memory buffering beyond chunk size).
pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(depot_path): Path<String>,
    body: axum::body::Body,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let depot_path = normalize_depot_path(&depot_path);

    let depot = Depot::new(&state.config.depot_root);
    depot.init().await.map_err(internal_error)?;

    // Prepare staging path
    let normalized = depot_path.trim_start_matches('/');
    let staging_path = depot.blob_path(&format!("staging_{}", normalized.replace('/', "_")));
    if let Some(parent) = staging_path.parent() {
        std::fs::create_dir_all(parent).map_err(internal_error)?;
    }

    // Stream body to blob + staging file, computing digest incrementally
    use sha2::{Sha256, Digest};
    use tokio::io::AsyncWriteExt;
    use futures_util::StreamExt;

    let mut body_stream = body.into_data_stream();
    let tmp_blob = std::env::temp_dir().join(format!("crv_upload_{}", uuid::Uuid::new_v4()));
    let mut blob_file = tokio::fs::File::create(&tmp_blob)
        .await
        .map_err(internal_error)?;
    let mut staging_file = tokio::fs::File::create(&staging_path)
        .await
        .map_err(internal_error)?;
    let mut hasher = Sha256::new();
    let mut total_size: u64 = 0;

    while let Some(chunk_result) = body_stream.next().await {
        let chunk = chunk_result.map_err(|e| bad_request(&format!("body read: {e}")))?;
        total_size += chunk.len() as u64;
        hasher.update(&chunk);

        // Write to both blob temp and staging file
        blob_file.write_all(&chunk).await.map_err(internal_error)?;
        staging_file.write_all(&chunk).await.map_err(internal_error)?;
    }

    if total_size == 0 {
        let _ = tokio::fs::remove_file(&tmp_blob).await;
        let _ = tokio::fs::remove_file(&staging_path).await;
        return Err(bad_request("empty file content"));
    }

    // Finalize: compute digest, move temp to final blob path
    let digest = hex::encode(hasher.finalize());
    let final_blob = depot.blob_path(&digest);

    if !final_blob.exists() {
        if let Some(parent) = final_blob.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(internal_error)?;
        }
        tokio::fs::rename(&tmp_blob, &final_blob).await.map_err(internal_error)?;
    } else {
        // Blob already exists (content dedup), discard temp
        tokio::fs::remove_file(&tmp_blob).await.ok();
    }

    drop(staging_file); // ensure flush

    Ok(Json(json!({
        "success": true,
        "data": {
            "depot_path": depot_path,
            "digest": digest,
            "size": total_size,
        }
    })))
}

// ── File Info ──────────────────────────────────────────────────────

/// GET /api/v1/files/:depot_path/fstat
pub async fn file_stat(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(depot_path): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let depot_path = normalize_depot_path(&depot_path);
    let row = sqlx::query(
        "SELECT depot_path, revision, action, file_type, digest, size, created_at
         FROM file_revisions WHERE depot_path = $1
         ORDER BY revision DESC LIMIT 1",
    )
    .bind(&depot_path)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(internal_error)?
    .ok_or_else(|| not_found("file not found"))?;

    Ok(Json(json!({"success": true, "data": {
        "depot_path": row.get::<String, _>("depot_path"),
        "head_revision": row.get::<i32, _>("revision"),
        "head_action": row.get::<String, _>("action"),
        "file_type": row.get::<String, _>("file_type"),
        "digest": row.get::<String, _>("digest"),
        "file_size": row.get::<i64, _>("size"),
        "head_time": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
    }})))
}

/// GET /api/v1/files/:depot_path/filelog
pub async fn file_log(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(depot_path): Path<String>,
    Query(q): Query<FileLogQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let depot_path = normalize_depot_path(&depot_path);
    let limit = q.max.unwrap_or(50);

    let rows = sqlx::query(
        "SELECT fr.revision, fr.action, fr.file_type, fr.digest, fr.size, fr.created_at,
                c.number as change_number, c.user_id, c.description
         FROM file_revisions fr
         JOIN changes c ON fr.change_id = c.id
         WHERE fr.depot_path = $1
         ORDER BY fr.revision DESC
         LIMIT $2",
    )
    .bind(&depot_path)
    .bind(limit)
    .fetch_all(&state.db_pool)
    .await
    .map_err(internal_error)?;

    let history: Vec<Value> = rows.iter().map(|r| json!({
        "revision": r.get::<i32, _>("revision"),
        "action": r.get::<String, _>("action"),
        "file_type": r.get::<String, _>("file_type"),
        "digest": r.get::<String, _>("digest"),
        "file_size": r.get::<i64, _>("size"),
        "time": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        "change": r.get::<i64, _>("change_number"),
        "user_id": r.get::<Uuid, _>("user_id"),
        "description": r.get::<String, _>("description"),
    })).collect();

    Ok(Json(json!({"success": true, "data": history})))
}

#[derive(Deserialize)]
pub struct FileLogQuery {
    pub max: Option<i64>,
}

// ── Sync ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SyncQuery {
    pub filespec: Option<String>,
    #[serde(default)]
    pub force: bool,
}

/// GET /api/v1/clients/:client/sync
pub async fn sync_client(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
    Query(q): Query<SyncQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;
    let depot = Depot::new(&state.config.depot_root);

    let entries = sync::compute_sync(
        &state.db_pool,
        &depot,
        client.id,
        q.filespec.as_deref(),
        q.force,
    )
    .await
    .map_err(internal_error)?;

    let total_bytes: i64 = entries.iter().map(|e| if e.needs_content { e.file_size } else { 0 }).sum();

    Ok(Json(json!({
        "success": true,
        "data": {
            "files": entries,
            "total_bytes": total_bytes,
        }
    })))
}

/// POST /api/v1/clients/:client/sync/confirm
pub async fn confirm_sync(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(client_name): Path<String>,
    Json(entries): Json<Vec<SyncConfirmEntry>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let client = resolve_client(&state, &client_name).await?;

    let sync_entries: Vec<crv_shared::types::SyncFileEntry> = entries
        .into_iter()
        .map(|e| crv_shared::types::SyncFileEntry {
            depot_path: e.depot_path,
            revision: e.revision,
            action: FileActionType::Edit,
            file_type: crv_shared::types::FileType::Text,
            file_size: e.file_size,
            digest: e.digest,
            needs_content: false,
        })
        .collect();

    sync::record_sync(&state.db_pool, client.id, &sync_entries)
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({"success": true, "data": {"synced": sync_entries.len()}})))
}

#[derive(Deserialize)]
pub struct SyncConfirmEntry {
    pub depot_path: String,
    pub revision: i32,
    pub digest: String,
    pub file_size: i64,
}

// ── Helpers ────────────────────────────────────────────────────────

struct ClientInfo {
    id: Uuid,
}

async fn resolve_client(
    state: &AppState,
    name: &str,
) -> Result<ClientInfo, (StatusCode, Json<Value>)> {
    let row = sqlx::query("SELECT id FROM clients WHERE name = $1")
        .bind(name)
        .fetch_optional(&state.db_pool)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("client not found"))?;
    Ok(ClientInfo { id: row.get("id") })
}

/// Resolve change_id: if None, get or create the default pending changelist.
async fn resolve_change(
    state: &AppState,
    client_id: Uuid,
    change_id: Option<Uuid>,
) -> Result<Option<Uuid>, (StatusCode, Json<Value>)> {
    if let Some(cid) = change_id {
        let valid = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM changes WHERE id = $1 AND client_id = $2 AND status = 'pending')",
        )
        .bind(cid)
        .bind(client_id)
        .fetch_one(&state.db_pool)
        .await
        .map_err(internal_error)?;
        if !valid {
            return Err(bad_request("changelist not found or not pending"));
        }
        Ok(Some(cid))
    } else {
        // Use default changelist (any pending without explicit number)
        Ok(None)
    }
}

fn internal_error<E: std::fmt::Display>(e: E) -> (StatusCode, Json<Value>) {
    tracing::error!("{e}");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"success": false, "error": format!("{e}")})))
}
fn not_found(msg: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({"success": false, "error": msg})))
}
fn bad_request(msg: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"success": false, "error": msg})))
}
