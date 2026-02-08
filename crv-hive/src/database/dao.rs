use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseBackend, DbErr, EntityTrait, Set, Statement,
    TransactionTrait,
};
use async_trait::async_trait;
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

// ============================
// Dao trait + implementations
// ============================

#[async_trait]
pub trait Dao: Send + Sync {
    async fn find_user_by_username(&self, username: &str) -> DaoResult<Option<entities::users::Model>>;
    async fn insert_user(&self, username: &str, password_hash: &str) -> DaoResult<()>;

    async fn find_latest_file_revision_by_depot_path(
        &self,
        depot_path: &str,
    ) -> DaoResult<Option<entities::file_revisions::Model>>;

    async fn insert_changelist(
        &self,
        author: &str,
        description: &str,
        committed_at: i64,
        metadata: serde_json::Value,
    ) -> DaoResult<i64>;

    async fn commit_submit(
        &self,
        author: &str,
        description: &str,
        committed_at: i64,
        metadata: serde_json::Value,
        revisions: Vec<NewFileRevisionInput>,
    ) -> DaoResult<i64>;
}

/// 生产实现：使用 SeaORM + 全局单例连接池（`crate::database::DB_CONN`）
#[derive(Debug, Default)]
pub struct SeaOrmDao;

#[async_trait]
impl Dao for SeaOrmDao {
    async fn find_user_by_username(
        &self,
        username: &str,
    ) -> DaoResult<Option<entities::users::Model>> {
        find_user_by_username_on(db()?, username).await
    }

    async fn insert_user(&self, username: &str, password_hash: &str) -> DaoResult<()> {
        insert_user_on(db()?, username, password_hash).await
    }

    async fn find_latest_file_revision_by_depot_path(
        &self,
        depot_path: &str,
    ) -> DaoResult<Option<entities::file_revisions::Model>> {
        find_latest_file_revision_by_depot_path_on(db()?, depot_path).await
    }

    async fn insert_changelist(
        &self,
        author: &str,
        description: &str,
        committed_at: i64,
        metadata: serde_json::Value,
    ) -> DaoResult<i64> {
        insert_changelist_on(db()?, author, description, committed_at, metadata).await
    }

    async fn commit_submit(
        &self,
        author: &str,
        description: &str,
        committed_at: i64,
        metadata: serde_json::Value,
        revisions: Vec<NewFileRevisionInput>,
    ) -> DaoResult<i64> {
        commit_submit_on(db()?, author, description, committed_at, metadata, revisions)
            .await
    }
}

/// 测试实现：纯内存版本，便于本地/单测运行（不依赖 Postgres）。
#[derive(Debug, Default)]
pub struct MockDao {
    inner: std::sync::Mutex<MockDaoState>,
}

#[derive(Debug)]
struct MockDaoState {
    next_changelist_id: i64,
    users: HashMap<String, entities::users::Model>,
    latest_revisions: HashMap<String, entities::file_revisions::Model>, // key: ltree_key
}

impl Default for MockDaoState {
    fn default() -> Self {
        Self {
            next_changelist_id: 1,
            users: HashMap::new(),
            latest_revisions: HashMap::new(),
        }
    }
}

#[async_trait]
impl Dao for MockDao {
    async fn find_user_by_username(
        &self,
        username: &str,
    ) -> DaoResult<Option<entities::users::Model>> {
        let g = self.inner.lock().expect("MockDao poisoned");
        Ok(g.users.get(username).cloned())
    }

    async fn insert_user(&self, username: &str, password_hash: &str) -> DaoResult<()> {
        let mut g = self.inner.lock().expect("MockDao poisoned");
        if g.users.contains_key(username) {
            return Err(DaoError::Db(DbErr::RecordNotInserted));
        }
        g.users.insert(
            username.to_string(),
            entities::users::Model {
                id: username.to_string(),
                password: password_hash.to_string(),
            },
        );
        Ok(())
    }

    async fn find_latest_file_revision_by_depot_path(
        &self,
        depot_path: &str,
    ) -> DaoResult<Option<entities::file_revisions::Model>> {
        let key = ltree_key::depot_path_str_to_ltree_key(depot_path)?;
        let g = self.inner.lock().expect("MockDao poisoned");
        Ok(g.latest_revisions.get(&key).cloned())
    }

    async fn insert_changelist(
        &self,
        _author: &str,
        _description: &str,
        _committed_at: i64,
        _metadata: serde_json::Value,
    ) -> DaoResult<i64> {
        let mut g = self.inner.lock().expect("MockDao poisoned");
        let id = g.next_changelist_id;
        g.next_changelist_id = g.next_changelist_id.saturating_add(1);
        Ok(id)
    }

    async fn commit_submit(
        &self,
        author: &str,
        description: &str,
        committed_at: i64,
        metadata: serde_json::Value,
        revisions: Vec<NewFileRevisionInput>,
    ) -> DaoResult<i64> {
        let changelist_id = self
            .insert_changelist(author, description, committed_at, metadata)
            .await?;

        let mut g = self.inner.lock().expect("MockDao poisoned");
        for r in revisions {
            let key = ltree_key::depot_path_str_to_ltree_key(&r.depot_path)?;

            let model = entities::file_revisions::Model {
                path: key.clone(),
                generation: r.generation,
                revision: r.revision,
                changelist_id,
                binary_id: r.binary_id,
                size: r.size,
                is_delete: r.is_delete,
                created_at: r.created_at,
                metadata: r.metadata,
            };

            // 更新 latest：按 (generation, revision) 取最大
            let should_replace = match g.latest_revisions.get(&key) {
                None => true,
                Some(existing) => {
                    (model.generation, model.revision) > (existing.generation, existing.revision)
                }
            };

            if should_replace {
                g.latest_revisions.insert(key, model);
            }
        }

        Ok(changelist_id)
    }
}

static DAO_INSTANCE: OnceLock<RwLock<Arc<dyn Dao>>> = OnceLock::new();

fn dao_cell() -> &'static RwLock<Arc<dyn Dao>> {
    DAO_INSTANCE.get_or_init(|| RwLock::new(Arc::new(SeaOrmDao::default())))
}

/// 获取当前 DAO（默认是生产实现 `SeaOrmDao`）。
pub fn dao() -> Arc<dyn Dao> {
    dao_cell()
        .read()
        .expect("dao RwLock poisoned")
        .clone()
}

/// 仅用于测试/本地：覆盖全局 DAO 实现（例如注入 `MockDao`）。
///
/// 注意：这是全局状态，建议测试串行使用（或自行加锁）。
pub fn set_dao_for_tests(new_dao: Arc<dyn Dao>) {
    *dao_cell().write().expect("dao RwLock poisoned") = new_dao;
}

async fn find_user_by_username_on<C: ConnectionTrait>(
    conn: &C,
    username: &str,
) -> DaoResult<Option<entities::users::Model>> {
    let model = entities::users::Entity::find_by_id(username.to_string())
        .one(conn)
        .await?;
    Ok(model)
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
    dao().find_user_by_username(username).await
}

/// 创建新用户文档。
///
/// - `username` 作为主键 `id` 字段；
/// - `password_hash` 存储为 `password` 字段，建议为 Argon2 哈希。
pub async fn insert_user(username: &str, password_hash: &str) -> DaoResult<()> {
    dao().insert_user(username, password_hash).await
}

async fn insert_user_on<C: ConnectionTrait>(
    conn: &C,
    username: &str,
    password_hash: &str,
) -> DaoResult<()> {
    let am = entities::users::ActiveModel {
        id: Set(username.to_string()),
        password: Set(password_hash.to_string()),
    };
    am.insert(conn).await?;
    Ok(())
}

/// 按 depot path 查询该文件的最新 revision（如果存在）。
///
/// 返回值为 `file_revisions` 的一条记录：按 `(generation desc, revision desc)` 取最大。
pub async fn find_latest_file_revision_by_depot_path(
    depot_path: &str,
) -> DaoResult<Option<entities::file_revisions::Model>> {
    dao().find_latest_file_revision_by_depot_path(depot_path).await
}

async fn find_latest_file_revision_by_depot_path_on<C: ConnectionTrait>(
    conn: &C,
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
        .one(conn)
        .await?;

    Ok(model)
}

/// 创建一个 changelist，并返回其自增 id。
pub async fn insert_changelist(
    author: &str,
    description: &str,
    committed_at: i64,
    metadata: serde_json::Value,
) -> DaoResult<i64> {
    dao()
        .insert_changelist(author, description, committed_at, metadata)
        .await
}

async fn insert_changelist_on<C: ConnectionTrait>(
    conn: &C,
    author: &str,
    description: &str,
    committed_at: i64,
    metadata: serde_json::Value,
) -> DaoResult<i64> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            INSERT INTO changelists (author, description, committed_at, metadata)
            VALUES ($1, $2, $3, $4::jsonb)
            RETURNING id
            "#,
            vec![
                author.to_string().into(),
                description.to_string().into(),
                committed_at.into(),
                metadata.to_string().into(),
            ],
        ))
        .await?;

    let row = row.ok_or_else(|| {
        DaoError::Db(DbErr::RecordNotFound(
            "failed to insert changelist".to_string(),
        ))
    })?;
    Ok(row.try_get("", "id")?)
}

async fn ensure_file_exists_on<C: ConnectionTrait>(
    conn: &C,
    depot_path: &str,
    created_at: i64,
    metadata: &serde_json::Value,
) -> DaoResult<()> {
    let key = ltree_key::depot_path_str_to_ltree_key(depot_path)?;
    conn.execute(Statement::from_sql_and_values(
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

async fn insert_file_revision_on<C: ConnectionTrait>(
    conn: &C,
    input: &NewFileRevisionInput,
    changelist_id: i64,
) -> DaoResult<()> {
    let key = ltree_key::depot_path_str_to_ltree_key(&input.depot_path)?;

    conn.execute(Statement::from_sql_and_values(
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
    metadata: serde_json::Value,
    revisions: Vec<NewFileRevisionInput>,
) -> DaoResult<i64> {
    dao()
        .commit_submit(author, description, committed_at, metadata, revisions)
        .await
}

async fn commit_submit_on(
    conn: &sea_orm::DatabaseConnection,
    author: &str,
    description: &str,
    committed_at: i64,
    metadata: serde_json::Value,
    revisions: Vec<NewFileRevisionInput>,
) -> DaoResult<i64> {
    let txn = conn.begin().await?;

    // 复用 DAO 的插入逻辑（只是在事务里执行）
    let changelist_id =
        insert_changelist_on(&txn, author, description, committed_at, metadata).await?;

    for r in &revisions {
        ensure_file_exists_on(&txn, &r.depot_path, r.created_at, &r.metadata).await?;
        insert_file_revision_on(&txn, r, changelist_id).await?;
    }

    txn.commit().await?;
    Ok(changelist_id)
}

#[cfg(test)]
mod dao_trait_tests {
    use super::*;

    #[tokio::test]
    async fn mock_dao_insert_and_find_user() {
        // 注意：这是全局覆盖，测试尽量保持简单。
        set_dao_for_tests(Arc::new(MockDao::default()));

        insert_user("alice", "hash").await.expect("insert user");
        let u = find_user_by_username("alice")
            .await
            .expect("find user")
            .expect("user should exist");
        assert_eq!(u.id, "alice");
        assert_eq!(u.password, "hash");
    }
}
