use crv_core::metadata::{
    BranchDoc, BranchMetadata, ChangelistChange, ChangelistDoc, ChangelistMetadata, FileDoc,
    FileMetadata, FileRevisionDoc, FileRevisionMetadata, UserDoc,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, DbErr, EntityTrait, QueryFilter,
    Set, Statement,
};
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

/// 根据用户名（`users._id`）查询用户文档。
pub async fn find_user_by_username(username: &str) -> DaoResult<Option<UserDoc>> {
    let model = entities::users::Entity::find_by_id(username.to_string())
        .one(db()?)
        .await?;
    Ok(model.map(|m| UserDoc {
        id: m.id,
        password: m.password,
    }))
}

/// 创建新用户文档。
///
/// - `username` 作为 MongoDB `_id` 字段；
/// - `password_hash` 存储为 `password` 字段，建议为 Argon2 哈希。
pub async fn insert_user(username: &str, password_hash: &str) -> DaoResult<()> {
    let am = entities::users::ActiveModel {
        id: Set(username.to_string()),
        password: Set(password_hash.to_string()),
    };
    am.insert(db()?).await?;
    Ok(())
}

/// 根据分支 ID 查询 Branch 文档。
pub async fn find_branch_by_id(branch_id: &str) -> DaoResult<Option<BranchDoc>> {
    let model = entities::branches::Entity::find_by_id(branch_id.to_string())
        .one(db()?)
        .await?;
    let Some(m) = model else { return Ok(None) };

    let metadata: BranchMetadata = serde_json::from_value(m.metadata)?;
    Ok(Some(BranchDoc {
        id: m.id,
        created_at: m.created_at,
        created_by: m.created_by,
        head_changelist_id: m.head_changelist_id,
        metadata,
    }))
}

/// 根据 changelist id 查询 Changelist 文档。
pub async fn find_changelist_by_id(changelist_id: i64) -> DaoResult<Option<ChangelistDoc>> {
    let model = entities::changelists::Entity::find_by_id(changelist_id)
        .one(db()?)
        .await?;
    let Some(m) = model else { return Ok(None) };

    let changes: Vec<ChangelistChange> = serde_json::from_value(m.changes)?;
    let metadata: ChangelistMetadata = serde_json::from_value(m.metadata)?;
    Ok(Some(ChangelistDoc {
        id: m.id,
        parent_changelist_id: m.parent_changelist_id,
        branch_id: m.branch_id,
        author: m.author,
        description: m.description,
        changes,
        committed_at: m.committed_at,
        files_count: m.files_count,
        metadata,
    }))
}

/// 查询指定分支、文件与 changelist 下的 FileRevision 文档。
pub async fn find_file_revision_by_branch_file_and_cl(
    branch_id: &str,
    file_id: &str,
    changelist_id: i64,
) -> DaoResult<Option<FileRevisionDoc>> {
    let model = entities::file_revisions::Entity::find()
        .filter(entities::file_revisions::Column::BranchId.eq(branch_id))
        .filter(entities::file_revisions::Column::FileId.eq(file_id))
        .filter(entities::file_revisions::Column::ChangelistId.eq(changelist_id))
        .one(db()?)
        .await?;
    let Some(m) = model else { return Ok(None) };

    let binary_id: Vec<String> = serde_json::from_value(m.binary_id)?;
    let metadata: FileRevisionMetadata = serde_json::from_value(m.metadata)?;
    Ok(Some(FileRevisionDoc {
        id: m.id,
        branch_id: m.branch_id,
        file_id: m.file_id,
        changelist_id: m.changelist_id,
        binary_id,
        parent_revision_id: m.parent_revision_id,
        size: m.size,
        is_delete: m.is_delete,
        created_at: m.created_at,
        metadata,
    }))
}

/// 根据 revision id 查询 FileRevision 文档。
pub async fn find_file_revision_by_id(revision_id: &str) -> DaoResult<Option<FileRevisionDoc>> {
    let model = entities::file_revisions::Entity::find_by_id(revision_id.to_string())
        .one(db()?)
        .await?;
    let Some(m) = model else { return Ok(None) };

    let binary_id: Vec<String> = serde_json::from_value(m.binary_id)?;
    let metadata: FileRevisionMetadata = serde_json::from_value(m.metadata)?;
    Ok(Some(FileRevisionDoc {
        id: m.id,
        branch_id: m.branch_id,
        file_id: m.file_id,
        changelist_id: m.changelist_id,
        binary_id,
        parent_revision_id: m.parent_revision_id,
        size: m.size,
        is_delete: m.is_delete,
        created_at: m.created_at,
        metadata,
    }))
}

/// 根据文件 ID 查询 File 文档。
pub async fn find_file_by_id(file_id: &str) -> DaoResult<Option<FileDoc>> {
    let model = entities::files::Entity::find_by_id(file_id.to_string())
        .one(db()?)
        .await?;
    let Some(m) = model else { return Ok(None) };

    let metadata: FileMetadata = serde_json::from_value(m.metadata)?;
    Ok(Some(FileDoc {
        id: m.id,
        path: m.path,
        created_at: m.created_at,
        metadata,
    }))
}

/// 插入新的 File 文档。
pub async fn insert_file(doc: FileDoc) -> DaoResult<()> {
    let am = entities::files::ActiveModel {
        id: Set(doc.id),
        path: Set(doc.path),
        created_at: Set(doc.created_at),
        metadata: Set(serde_json::to_value(doc.metadata)?),
    };
    am.insert(db()?).await?;
    Ok(())
}

/// 插入一批 FileRevision 文档。
pub async fn insert_file_revisions(docs: Vec<FileRevisionDoc>) -> DaoResult<()> {
    if docs.is_empty() {
        return Ok(());
    }
    let active_models = docs
        .into_iter()
        .map(|d| {
            Ok(entities::file_revisions::ActiveModel {
                id: Set(d.id),
                branch_id: Set(d.branch_id),
                file_id: Set(d.file_id),
                changelist_id: Set(d.changelist_id),
                binary_id: Set(serde_json::to_value(d.binary_id)?),
                parent_revision_id: Set(d.parent_revision_id),
                size: Set(d.size),
                is_delete: Set(d.is_delete),
                created_at: Set(d.created_at),
                metadata: Set(serde_json::to_value(d.metadata)?),
            })
        })
        .collect::<Result<Vec<_>, DaoError>>()?;

    entities::file_revisions::Entity::insert_many(active_models)
        .exec(db()?)
        .await?;
    Ok(())
}

/// 插入新的 Changelist 文档。
pub async fn insert_changelist(doc: ChangelistDoc) -> DaoResult<()> {
    let am = entities::changelists::ActiveModel {
        id: Set(doc.id),
        parent_changelist_id: Set(doc.parent_changelist_id),
        branch_id: Set(doc.branch_id),
        author: Set(doc.author),
        description: Set(doc.description),
        changes: Set(serde_json::to_value(doc.changes)?),
        committed_at: Set(doc.committed_at),
        files_count: Set(doc.files_count),
        metadata: Set(serde_json::to_value(doc.metadata)?),
    };
    am.insert(db()?).await?;
    Ok(())
}

/// 更新指定分支的 HEAD changelist id。
pub async fn update_branch_head(branch_id: &str, new_head: i64) -> DaoResult<()> {
    use sea_orm::sea_query::Expr;

    entities::branches::Entity::update_many()
        .filter(entities::branches::Column::Id.eq(branch_id))
        .col_expr(
            entities::branches::Column::HeadChangelistId,
            Expr::value(new_head),
        )
        .exec(db()?)
        .await?;
    Ok(())
}

/// 获取当前最大 changelist id（调试/兼容用途；生产不建议用它来分配新 ID）。
pub async fn get_max_changelist_id() -> DaoResult<i64> {
    let stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT COALESCE(MAX(id), 0) AS max_id FROM changelists".to_string(),
    );
    let row = db()?.query_one(stmt).await?;
    let max_id: i64 = row
        .ok_or_else(|| DbErr::RecordNotFound("max(id) returned no row".to_string()))?
        .try_get("", "max_id")?;
    Ok(max_id)
}

/// 分配一个新的 changelist id（基于 Postgres 序列，适配 BIGSERIAL/IDENTITY）。
pub async fn allocate_changelist_id() -> DaoResult<i64> {
    let stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT nextval(pg_get_serial_sequence('changelists','id')) AS id".to_string(),
    );
    let row = db()?.query_one(stmt).await?;
    let id: i64 = row
        .ok_or_else(|| DbErr::RecordNotFound("nextval returned no row".to_string()))?
        .try_get("", "id")?;
    Ok(id)
}

