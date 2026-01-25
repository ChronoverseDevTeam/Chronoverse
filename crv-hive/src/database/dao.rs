use sea_orm::{
    ActiveModelTrait, DbErr, EntityTrait, Set, Statement, DatabaseBackend,
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

    // `file_revisions.path` 是 Postgres `ltree`，而 SeaORM 这里字段类型用 `String`。
    // 在某些 Postgres 版本/配置下，`ltree = text` 不会隐式 cast，导致查询报错。
    // 这里用 raw SQL 显式 `$1::ltree`，避免类型不匹配。
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
        SELECT
            path::text AS path,
            generation,
            revision,
            changelist_id,
            binary_id,
            size,
            is_delete,
            created_at,
            metadata
        FROM file_revisions
        WHERE path = $1::ltree
        ORDER BY generation DESC, revision DESC
        LIMIT 1
        "#,
        [key.into()].to_vec(),
    );

    let model = entities::file_revisions::Entity::find()
        .from_raw_sql(stmt)
        .one(db()?)
        .await?;

    Ok(model)
}
