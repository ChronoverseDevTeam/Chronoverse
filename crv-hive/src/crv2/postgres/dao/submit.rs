use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait,
    QueryFilter, QueryOrder, Set,
};

use crate::crv2::postgres::entity::submit::{ActiveModel, Column, Entity, Model};
use crate::crv2::postgres::entity::submit_file;
use super::DaoResult;

/// Input for creating a new submit.
pub struct NewSubmit {
    pub author: String,
    pub description: String,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
    /// Unix timestamp in milliseconds — lock expiry deadline.
    pub expires_at: i64,
}

/// Input for adding a file to a submit.
pub struct NewSubmitFile {
    pub submit_id: i64,
    pub path: String,
    /// add | edit | delete
    pub action: String,
    pub chunk_hashes: Vec<String>,
    pub size: i64,
}

// ── Submit queries ───────────────────────────────────────────────────────────

/// Find a submit by its ID.
pub async fn find_by_id(db: &impl ConnectionTrait, id: i64) -> DaoResult<Option<Model>> {
    Ok(Entity::find_by_id(id).one(db).await?)
}

/// Return all pending submits by `author`, ordered newest first.
pub async fn find_pending_by_author(
    db: &impl ConnectionTrait,
    author: &str,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Author.eq(author))
        .filter(Column::Status.eq("pending"))
        .order_by_desc(Column::Id)
        .all(db)
        .await?)
}

/// Return all submits that have expired but are still pending.
/// These are candidates for automatic expiration.
pub async fn find_expired_pending(
    db: &impl ConnectionTrait,
    now_ms: i64,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Status.eq("pending"))
        .filter(Column::ExpiresAt.lte(now_ms))
        .all(db)
        .await?)
}

/// Return all pending submits that have NOT yet expired.
/// Used by the blob-event listener to extend their expiry on activity.
pub async fn find_pending_active(
    db: &impl ConnectionTrait,
    now_ms: i64,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Status.eq("pending"))
        .filter(Column::ExpiresAt.gt(now_ms))
        .all(db)
        .await?)
}

// ── Submit writes ────────────────────────────────────────────────────────────

/// Create a new submit in `pending` status. Returns the auto-assigned ID.
pub async fn create(db: &impl ConnectionTrait, new: NewSubmit) -> DaoResult<i64> {
    let am = ActiveModel {
        author: Set(new.author),
        description: Set(new.description),
        status: Set("pending".to_string()),
        changelist_id: Set(None),
        created_at: Set(new.created_at),
        expires_at: Set(new.expires_at),
        ..Default::default()
    };
    let result = am.insert(db).await?;
    Ok(result.id)
}

/// Transition a pending submit to `committed` and link it to a changelist.
pub async fn mark_committed(
    db: &impl ConnectionTrait,
    submit_id: i64,
    changelist_id: i64,
) -> DaoResult<()> {
    let am = ActiveModel {
        id: Set(submit_id),
        status: Set("committed".to_string()),
        changelist_id: Set(Some(changelist_id)),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

/// Transition a pending submit to `cancelled`, releasing all file locks.
pub async fn mark_cancelled(db: &impl ConnectionTrait, submit_id: i64) -> DaoResult<()> {
    let am = ActiveModel {
        id: Set(submit_id),
        status: Set("cancelled".to_string()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

/// Transition a pending submit to `expired`, releasing all file locks.
pub async fn mark_expired(db: &impl ConnectionTrait, submit_id: i64) -> DaoResult<()> {
    let am = ActiveModel {
        id: Set(submit_id),
        status: Set("expired".to_string()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

/// Update the description of a pending submit.
pub async fn update_description(
    db: &impl ConnectionTrait,
    submit_id: i64,
    description: String,
) -> DaoResult<()> {
    let am = ActiveModel {
        id: Set(submit_id),
        description: Set(description),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

/// Extend the expiry deadline of a pending submit (heartbeat / keep-alive).
pub async fn extend_expiry(
    db: &impl ConnectionTrait,
    submit_id: i64,
    new_expires_at: i64,
) -> DaoResult<()> {
    if let Some(model) = find_by_id(db, submit_id).await? {
        if model.status != "pending" || model.expires_at <= current_time_millis() {
            return Ok(());
        }
    } else {
        return Ok(());
    }

    let am = ActiveModel {
        id: Set(submit_id),
        expires_at: Set(new_expires_at),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

// ── Submit file queries ──────────────────────────────────────────────────────

/// Return all files belonging to a submit.
pub async fn find_files(
    db: &impl ConnectionTrait,
    submit_id: i64,
) -> DaoResult<Vec<submit_file::Model>> {
    Ok(submit_file::Entity::find()
        .filter(submit_file::Column::SubmitId.eq(submit_id))
        .all(db)
        .await?)
}

/// Check whether any of the given depot paths are locked by a pending submit
/// (other than `exclude_submit_id`). Returns the paths that are locked.
///
/// This is the core pessimistic-lock query: if the returned vec is non-empty,
/// the caller must reject the operation.
pub async fn find_locked_paths(
    db: &impl ConnectionTrait,
    paths: &[&str],
    exclude_submit_id: Option<i64>,
) -> DaoResult<Vec<String>> {
    use sea_orm::QuerySelect;

    if paths.is_empty() {
        return Ok(Vec::new());
    }

    // Join submit_files with submits to filter by pending status.
    let mut query = submit_file::Entity::find()
        .inner_join(Entity)
        .filter(submit_file::Column::Path.is_in(paths.iter().copied()))
        .filter(Column::Status.eq("pending"))
        .filter(Column::ExpiresAt.gt(current_time_millis()));

    if let Some(exclude_id) = exclude_submit_id {
        query = query.filter(submit_file::Column::SubmitId.ne(exclude_id));
    }

    let locked: Vec<submit_file::Model> = query
        .select_only()
        .columns([submit_file::Column::Path, submit_file::Column::SubmitId])
        .column(submit_file::Column::Action)
        .column(submit_file::Column::ChunkHashes)
        .column(submit_file::Column::Size)
        .into_model::<submit_file::Model>()
        .all(db)
        .await?;

    Ok(locked.into_iter().map(|m| m.path).collect())
}

fn current_time_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Submit file writes ───────────────────────────────────────────────────────

/// Add files to a submit. Typically called right after creating the submit.
pub async fn add_files(
    db: &impl ConnectionTrait,
    files: Vec<NewSubmitFile>,
) -> DaoResult<()> {
    if files.is_empty() {
        return Ok(());
    }

    let models: Vec<submit_file::ActiveModel> = files
        .into_iter()
        .map(|f| -> DaoResult<submit_file::ActiveModel> {
            Ok(submit_file::ActiveModel {
                submit_id: Set(f.submit_id),
                path: Set(f.path),
                action: Set(f.action),
                chunk_hashes: Set(serde_json::to_value(f.chunk_hashes)?),
                size: Set(f.size),
            })
        })
        .collect::<DaoResult<Vec<_>>>()?;

    submit_file::Entity::insert_many(models).exec(db).await?;
    Ok(())
}
