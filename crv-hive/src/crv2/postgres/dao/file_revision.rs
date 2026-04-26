use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait,
    QueryFilter, QueryOrder, Set,
};

use crate::crv2::postgres::entity::file_revision::{ActiveModel, Column, Entity, Model};
use super::DaoResult;

/// Input for inserting a new file revision.
pub struct NewFileRevision {
    pub path: String,
    pub generation: i64,
    pub revision: i64,
    pub changelist_id: i64,
    /// Content-addressable chunk hashes that compose this revision.
    pub chunk_hashes: Vec<String>,
    /// Total uncompressed size in bytes. `0` for a deletion.
    pub size: i64,
    pub is_deletion: bool,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

fn into_active_model(r: NewFileRevision) -> DaoResult<ActiveModel> {
    Ok(ActiveModel {
        path: Set(r.path),
        generation: Set(r.generation),
        revision: Set(r.revision),
        changelist_id: Set(r.changelist_id),
        chunk_hashes: Set(serde_json::to_value(r.chunk_hashes)?),
        size: Set(r.size),
        is_deletion: Set(r.is_deletion),
        created_at: Set(r.created_at),
    })
}

// ── Point queries ────────────────────────────────────────────────────────────

/// Fetch the exact revision identified by the composite primary key
/// `(path, generation, revision)`. Returns `None` if not found.
pub async fn find_exact(
    db: &impl ConnectionTrait,
    path: &str,
    generation: i64,
    revision: i64,
) -> DaoResult<Option<Model>> {
    Ok(Entity::find()
        .filter(Column::Path.eq(path))
        .filter(Column::Generation.eq(generation))
        .filter(Column::Revision.eq(revision))
        .one(db)
        .await?)
}

/// Return the most recent revision for `path`
/// (highest `generation`, then highest `revision`).
pub async fn find_latest_by_path(
    db: &impl ConnectionTrait,
    path: &str,
) -> DaoResult<Option<Model>> {
    Ok(Entity::find()
        .filter(Column::Path.eq(path))
        .order_by_desc(Column::Generation)
        .order_by_desc(Column::Revision)
        .one(db)
        .await?)
}

/// Return the latest revision of `path` that was committed at or before
/// `changelist_id`. This is the core "sync to CL" query.
pub async fn find_latest_at_changelist(
    db: &impl ConnectionTrait,
    path: &str,
    changelist_id: i64,
) -> DaoResult<Option<Model>> {
    Ok(Entity::find()
        .filter(Column::Path.eq(path))
        .filter(Column::ChangelistId.lte(changelist_id))
        .order_by_desc(Column::Generation)
        .order_by_desc(Column::Revision)
        .one(db)
        .await?)
}

// ── Collection queries ───────────────────────────────────────────────────────

/// Return the full revision history for `path`, ordered oldest first.
pub async fn find_all_by_path(
    db: &impl ConnectionTrait,
    path: &str,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Path.eq(path))
        .order_by_asc(Column::Generation)
        .order_by_asc(Column::Revision)
        .all(db)
        .await?)
}

/// Return all revisions that belong to a given changelist.
pub async fn find_by_changelist(
    db: &impl ConnectionTrait,
    changelist_id: i64,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::ChangelistId.eq(changelist_id))
        .all(db)
        .await?)
}

/// Return all revisions whose path is under the given depot prefix.
///
/// The prefix should end with `/` when representing a directory, for example
/// `//depot/main/src/`.
pub async fn find_by_prefix_in_range(
    db: &impl ConnectionTrait,
    prefix: &str,
    from_changelist: Option<i64>,
    to_changelist: Option<i64>,
) -> DaoResult<Vec<Model>> {
    let pattern = format!("{}%", escape_like(prefix));
    let mut query = Entity::find()
        .filter(Column::Path.like(pattern))
        .order_by_desc(Column::ChangelistId)
        .order_by_asc(Column::Path)
        .order_by_desc(Column::Generation)
        .order_by_desc(Column::Revision);

    if let Some(from_id) = from_changelist {
        query = query.filter(Column::ChangelistId.gte(from_id));
    }
    if let Some(to_id) = to_changelist {
        query = query.filter(Column::ChangelistId.lte(to_id));
    }

    Ok(query.all(db).await?)
}

/// For each path in `paths`, return its latest revision.
///
/// Performs a single batch query and deduplicates in Rust.
/// Paths not found in the database are absent from the returned map.
pub async fn find_latest_for_paths(
    db: &impl ConnectionTrait,
    paths: &[&str],
) -> DaoResult<HashMap<String, Model>> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    // Ordering ensures the first occurrence of each path in the result is the latest.
    let models = Entity::find()
        .filter(Column::Path.is_in(paths.iter().copied()))
        .order_by_asc(Column::Path)
        .order_by_desc(Column::Generation)
        .order_by_desc(Column::Revision)
        .all(db)
        .await?;

    let mut map: HashMap<String, Model> = HashMap::new();
    for m in models {
        map.entry(m.path.clone()).or_insert(m);
    }
    Ok(map)
}

/// Like `find_latest_for_paths` but only considers revisions committed at or
/// before `changelist_id`. Useful for bulk "sync to CL" operations.
pub async fn find_latest_for_paths_at(
    db: &impl ConnectionTrait,
    paths: &[&str],
    changelist_id: i64,
) -> DaoResult<HashMap<String, Model>> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    let models = Entity::find()
        .filter(Column::Path.is_in(paths.iter().copied()))
        .filter(Column::ChangelistId.lte(changelist_id))
        .order_by_asc(Column::Path)
        .order_by_desc(Column::Generation)
        .order_by_desc(Column::Revision)
        .all(db)
        .await?;

    let mut map: HashMap<String, Model> = HashMap::new();
    for m in models {
        map.entry(m.path.clone()).or_insert(m);
    }
    Ok(map)
}

// ── Writes ───────────────────────────────────────────────────────────────────

/// Insert a single file revision.
pub async fn insert(db: &impl ConnectionTrait, revision: NewFileRevision) -> DaoResult<()> {
    into_active_model(revision)?.insert(db).await?;
    Ok(())
}

/// Insert multiple file revisions in one round-trip.
pub async fn insert_many(
    db: &impl ConnectionTrait,
    revisions: Vec<NewFileRevision>,
) -> DaoResult<()> {
    if revisions.is_empty() {
        return Ok(());
    }
    let models = revisions
        .into_iter()
        .map(into_active_model)
        .collect::<DaoResult<Vec<_>>>()?;
    Entity::insert_many(models).exec(db).await?;
    Ok(())
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}
