use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseBackend, DbErr, EntityTrait, Set, Statement,
    TransactionTrait,
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

#[derive(Debug, Clone)]
pub struct NewFileRevisionInput {
    pub depot_path: String,
    pub generation: i64,
    pub revision: i64,
    pub binary_id: serde_json::Value,
    pub size: i64,
    pub is_delete: bool,
    pub created_at: i64,
    pub metadata: serde_json::Value,
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

/// 创建一个 changelist，并返回其自增 id。
pub async fn insert_changelist(
    author: &str,
    description: &str,
    changes: serde_json::Value,
    committed_at: i64,
    metadata: serde_json::Value,
) -> DaoResult<i64> {
    let am = entities::changelists::ActiveModel {
        author: Set(author.to_string()),
        description: Set(description.to_string()),
        changes: Set(changes),
        committed_at: Set(committed_at),
        metadata: Set(metadata),
        ..Default::default()
    };

    let model = am.insert(db()?).await?;
    Ok(model.id)
}

async fn ensure_file_exists_in_txn(
    txn: &sea_orm::DatabaseTransaction,
    depot_path: &str,
    created_at: i64,
    metadata: serde_json::Value,
) -> DaoResult<()> {
    let key = ltree_key::depot_path_str_to_ltree_key(depot_path)?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
        INSERT INTO files (path, created_at, metadata)
        VALUES ($1::ltree, $2, $3::jsonb)
        ON CONFLICT (path) DO NOTHING
        "#,
        vec![key.into(), created_at.into(), metadata.to_string().into()],
    ))
    .await?;
    Ok(())
}

async fn insert_file_revision_in_txn(
    txn: &sea_orm::DatabaseTransaction,
    input: &NewFileRevisionInput,
    changelist_id: i64,
) -> DaoResult<()> {
    let key = ltree_key::depot_path_str_to_ltree_key(&input.depot_path)?;

    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
        INSERT INTO file_revisions
            (path, generation, revision, changelist_id, binary_id, size, is_delete, created_at, metadata)
        VALUES
            ($1::ltree, $2, $3, $4, $5::jsonb, $6, $7, $8, $9::jsonb)
        "#,
        vec![
            key.into(),
            input.generation.into(),
            input.revision.into(),
            changelist_id.into(),
            input.binary_id.to_string().into(),
            input.size.into(),
            input.is_delete.into(),
            input.created_at.into(),
            input.metadata.to_string().into(),
        ],
    ))
    .await?;

    Ok(())
}

/// 原子提交：
/// - 创建 changelist
/// - 确保 files 行存在
/// - 写入每个文件的 file_revisions
pub async fn commit_submit(
    author: &str,
    description: &str,
    committed_at: i64,
    changes: serde_json::Value,
    metadata: serde_json::Value,
    revisions: Vec<NewFileRevisionInput>,
) -> DaoResult<i64> {
    let conn = db()?;
    let txn = conn.begin().await?;

    let changelist_id = {
        let am = entities::changelists::ActiveModel {
            author: Set(author.to_string()),
            description: Set(description.to_string()),
            changes: Set(changes),
            committed_at: Set(committed_at),
            metadata: Set(metadata),
            ..Default::default()
        };
        let model = am.insert(&txn).await?;
        model.id
    };

    for r in &revisions {
        ensure_file_exists_in_txn(&txn, &r.depot_path, r.created_at, r.metadata.clone()).await?;
        insert_file_revision_in_txn(&txn, r, changelist_id).await?;
    }

    txn.commit().await?;
    Ok(changelist_id)
}
