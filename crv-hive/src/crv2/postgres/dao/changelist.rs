use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};

use crate::crv2::postgres::entity::changelist::{ActiveModel, Column, Entity, Model};
use super::DaoResult;

/// Input for creating a new changelist.
pub struct NewChangelist {
    pub author: String,
    pub description: String,
    /// Unix timestamp in milliseconds.
    pub committed_at: i64,
}

/// Look up a changelist by its ID. Returns `None` if not found.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> DaoResult<Option<Model>> {
    Ok(Entity::find_by_id(id).one(db).await?)
}

/// Return the `limit` most recent changelists, ordered newest first.
pub async fn list_latest(db: &DatabaseConnection, limit: u64) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .order_by_desc(Column::Id)
        .limit(limit)
        .all(db)
        .await?)
}

/// Return all changelists submitted by `author`, ordered newest first.
pub async fn find_by_author(db: &DatabaseConnection, author: &str) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Author.eq(author))
        .order_by_desc(Column::Id)
        .all(db)
        .await?)
}

/// Return changelists with `id >= since_id`, ordered oldest first, up to `limit`.
///
/// Useful for incremental sync: repeatedly call with the last seen ID + 1.
pub async fn find_since(
    db: &DatabaseConnection,
    since_id: i64,
    limit: u64,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Id.gte(since_id))
        .order_by_asc(Column::Id)
        .limit(limit)
        .all(db)
        .await?)
}

/// Return changelists in the inclusive range `[from_id, to_id]`, ordered by id ascending.
pub async fn find_range(
    db: &DatabaseConnection,
    from_id: i64,
    to_id: i64,
) -> DaoResult<Vec<Model>> {
    Ok(Entity::find()
        .filter(Column::Id.between(from_id, to_id))
        .order_by_asc(Column::Id)
        .all(db)
        .await?)
}

/// Insert a new changelist and return the auto-assigned ID.
pub async fn insert(db: &DatabaseConnection, new_cl: NewChangelist) -> DaoResult<i64> {
    let am = ActiveModel {
        author: Set(new_cl.author),
        description: Set(new_cl.description),
        committed_at: Set(new_cl.committed_at),
        ..Default::default()
    };
    let result = am.insert(db).await?;
    Ok(result.id)
}
