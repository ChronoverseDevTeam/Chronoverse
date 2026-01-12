use sea_orm::{
    ActiveModelTrait, ColumnTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, Set,
};
use thiserror::Error;

use crate::database::entities;
use crate::database::ltree_key;

/// DAO 层错误类型
#[derive(Debug, Error)]
pub enum DaoError {
    #[error("Database is not initialized")]
    DatabaseNotInitialized,

    #[error("Database error: {0}")]
    Db(#[from] DbErr),

    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Ltree key error: {0}")]
    LtreeKey(#[from] ltree_key::LtreeKeyError),
}

pub type DaoResult<T> = Result<T, DaoError>;

fn db() -> DaoResult<&'static sea_orm::DatabaseConnection> {
    crate::database::try_get().ok_or(DaoError::DatabaseNotInitialized)
}

/// 根据用户名查找用户。
pub async fn find_user_by_username(username: &str) -> DaoResult<Option<entities::users::Model>> {
    let model = entities::users::Entity::find_by_id(username.to_string())
        .one(db()?)
        .await?;
    
    Ok(model)
}

/// 创建新用户文档。
///
/// - `username` 作为主键 `id` 字段；
/// - `password_hash` 存储为 `password` 字段，建议为 Argon2 哈希。
pub async fn insert_user(username: &str, password_hash: &str) -> DaoResult<()> {
    let am = entities::users::ActiveModel {
        id: Set(username.to_string()),
        password: Set(password_hash.to_string()),
    };
    am.insert(db()?).await?;
    Ok(())
}

/// 按 depot path 查询该文件的最新 revision（如果存在）。
///
/// 返回值为 `file_revisions` 的一条记录：按 `(generation desc, revision desc)` 取最大。
pub async fn find_latest_file_revision_by_depot_path(
    depot_path: &str,
) -> DaoResult<Option<entities::file_revisions::Model>> {
    let key = ltree_key::depot_path_str_to_ltree_key(depot_path)?;

    let model = entities::file_revisions::Entity::find()
        .filter(entities::file_revisions::Column::Path.eq(key))
        .order_by_desc(entities::file_revisions::Column::Generation)
        .order_by_desc(entities::file_revisions::Column::Revision)
        .one(db()?)
        .await?;

    Ok(model)
}
