use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};
use sea_orm::sea_query::OnConflict;

use crate::crv2::postgres::entity::file::{ActiveModel, Column, Entity, Model};
use super::{DaoError, DaoResult};

/// Look up a file record by its depot path. Returns `None` if not found.
pub async fn find_by_path(db: &DatabaseConnection, path: &str) -> DaoResult<Option<Model>> {
    Ok(Entity::find_by_id(path.to_owned()).one(db).await?)
}

/// Return all file records whose depot path starts with `prefix`.
///
/// `prefix` should be a depot directory path, e.g. `//depot/main/`.  
/// The trailing `/` is important so that `//depot/main/foo` matches but  
/// `//depot/mainline/bar` does not.
pub async fn find_by_prefix(
    db: &DatabaseConnection,
    prefix: &str,
) -> DaoResult<Vec<Model>> {
    // Escape SQL LIKE special characters in the prefix, then append `%`.
    let pattern = format!("{}%", escape_like(prefix));
    Ok(Entity::find()
        .filter(Column::Path.like(pattern))
        .all(db)
        .await?)
}

/// Ensure the depot `path` exists in the `files` table.
///
/// If the path is already present the existing record is left untouched
/// (`ON CONFLICT DO NOTHING`). `created_at` is only written on the first insert.
pub async fn upsert(db: &DatabaseConnection, path: &str, created_at: i64) -> DaoResult<()> {
    let am = ActiveModel {
        path: Set(path.to_owned()),
        created_at: Set(created_at),
    };
    match Entity::insert(am)
        .on_conflict(OnConflict::column(Column::Path).do_nothing().to_owned())
        .exec(db)
        .await
    {
        Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
        Err(e) => Err(DaoError::Db(e)),
    }
}

/// Delete a file record. Returns `true` if a row was removed.
///
/// Note: this will cascade-delete all associated `file_revisions` rows.
pub async fn delete(db: &DatabaseConnection, path: &str) -> DaoResult<bool> {
    let result = Entity::delete_by_id(path.to_owned()).exec(db).await?;
    Ok(result.rows_affected > 0)
}

/// Escape SQL LIKE metacharacters (`%`, `_`, `\`) in a literal string.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}
