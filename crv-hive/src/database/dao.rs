use sea_orm::{ActiveModelTrait, DbErr, EntityTrait, Set};
use thiserror::Error;

use crate::database::entities;

/// DAO 层错误类型
#[derive(Debug, Error)]
pub enum DaoError {
    #[error("Database is not initialized")]
    DatabaseNotInitialized,

    #[error("Database error: {0}")]
    Db(#[from] DbErr),

    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
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
