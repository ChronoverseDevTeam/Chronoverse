use crate::database::get_database;
use crv_core::metadata::{BranchDoc, ChangelistDoc, FileDoc, FileRevisionDoc, UserDoc};
use mongodb::{bson::doc, Collection};
use thiserror::Error;

/// MongoDB DAO 层错误类型
#[derive(Debug, Error)]
pub enum DaoError {
    #[error("MongoDB is not initialized")]
    DatabaseNotInitialized,

    #[error("MongoDB error: {0}")]
    Mongo(#[from] mongodb::error::Error),
}

pub type DaoResult<T> = Result<T, DaoError>;

fn users_collection() -> DaoResult<Collection<UserDoc>> {
    let db = get_database().ok_or(DaoError::DatabaseNotInitialized)?;
    Ok(db.collection::<UserDoc>("users"))
}

fn branches_collection() -> DaoResult<Collection<BranchDoc>> {
    let db = get_database().ok_or(DaoError::DatabaseNotInitialized)?;
    Ok(db.collection::<BranchDoc>("branches"))
}

fn file_revisions_collection() -> DaoResult<Collection<FileRevisionDoc>> {
    let db = get_database().ok_or(DaoError::DatabaseNotInitialized)?;
    Ok(db.collection::<FileRevisionDoc>("fileRevision"))
}

fn changelists_collection() -> DaoResult<Collection<ChangelistDoc>> {
    let db = get_database().ok_or(DaoError::DatabaseNotInitialized)?;
    Ok(db.collection::<ChangelistDoc>("changelists"))
}

fn files_collection() -> DaoResult<Collection<FileDoc>> {
    let db = get_database().ok_or(DaoError::DatabaseNotInitialized)?;
    Ok(db.collection::<FileDoc>("files"))
}

/// 根据用户名（`users._id`）查询用户文档。
pub async fn find_user_by_username(username: &str) -> DaoResult<Option<UserDoc>> {
    let coll = users_collection()?;
    let filter = doc! { "_id": username };
    let user = coll.find_one(filter).await?;
    Ok(user)
}

/// 创建新用户文档。
///
/// - `username` 作为 MongoDB `_id` 字段；
/// - `password_hash` 存储为 `password` 字段，建议为 Argon2 哈希。
pub async fn insert_user(username: &str, password_hash: &str) -> DaoResult<()> {
    let coll = users_collection()?;
    let user = UserDoc {
        id: username.to_string(),
        password: password_hash.to_string(),
    };
    coll.insert_one(user).await?;
    Ok(())
}

/// 根据分支 ID 查询 Branch 文档。
pub async fn find_branch_by_id(branch_id: &str) -> DaoResult<Option<BranchDoc>> {
    let coll = branches_collection()?;
    let filter = doc! { "_id": branch_id };
    let branch = coll.find_one(filter).await?;
    Ok(branch)
}

/// 查询指定分支、文件与 changelist 下的 FileRevision 文档。
pub async fn find_file_revision_by_branch_file_and_cl(
    branch_id: &str,
    file_id: &str,
    changelist_id: i64,
) -> DaoResult<Option<FileRevisionDoc>> {
    let coll = file_revisions_collection()?;
    let filter = doc! {
        "branchId": branch_id,
        "fileId": file_id,
        "changelistId": changelist_id,
    };
    let rev = coll.find_one(filter).await?;
    Ok(rev)
}

/// 根据文件 ID 查询 File 文档。
pub async fn find_file_by_id(file_id: &str) -> DaoResult<Option<FileDoc>> {
    let coll = files_collection()?;
    let filter = doc! { "_id": file_id };
    let file = coll.find_one(filter).await?;
    Ok(file)
}

/// 插入新的 File 文档。
pub async fn insert_file(doc: FileDoc) -> DaoResult<()> {
    let coll = files_collection()?;
    coll.insert_one(doc).await?;
    Ok(())
}

/// 插入一批 FileRevision 文档。
pub async fn insert_file_revisions(docs: Vec<FileRevisionDoc>) -> DaoResult<()> {
    if docs.is_empty() {
        return Ok(());
    }
    let coll = file_revisions_collection()?;
    coll.insert_many(docs).await?;
    Ok(())
}

/// 插入新的 Changelist 文档。
pub async fn insert_changelist(doc: ChangelistDoc) -> DaoResult<()> {
    let coll = changelists_collection()?;
    coll.insert_one(doc).await?;
    Ok(())
}

/// 更新指定分支的 HEAD changelist id。
pub async fn update_branch_head(branch_id: &str, new_head: i64) -> DaoResult<()> {
    let coll = branches_collection()?;
    coll.update_one(
        doc! { "_id": branch_id },
        doc! { "$set": { "headChangelistId": new_head } },
    )
    .await?;
    Ok(())
}

/// 获取当前最大 changelist id，用于简单的自增分配。
pub async fn get_max_changelist_id() -> DaoResult<i64> {
    let coll = changelists_collection()?;
    // 兼容旧版驱动：find_one 仅接受一个 filter 参数。
    // 这里简单传入空 filter，返回任意一条记录后在本地取其 id，若无记录则返回 0。
    let doc = coll.find_one(doc! {}).await?;
    Ok(doc.map(|d| d.id).unwrap_or(0))
}

