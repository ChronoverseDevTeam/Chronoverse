use crate::common::depot_path::DepotPath;
use crate::database::{
    dao::DaoError,
    entities::{changelists, file_revisions},
    ltree_key,
};
use sea_orm::{
    ColumnTrait, DatabaseBackend, DbErr, EntityTrait, ModelTrait, QueryFilter, QueryOrder,
    Statement, TransactionTrait,
};
use std::collections::HashMap;


fn db() -> Result<&'static sea_orm::DatabaseConnection, DaoError> {
    crate::database::try_get().ok_or(DaoError::DatabaseNotInitialized)
}

/// 按 changelist id 查询 changelist 及其关联的所有 file_revisions。
///
/// 这依赖 SeaORM entity 关系：
/// - `file_revisions` belongs_to `changelists`
/// - `changelists` has_many `file_revisions`
pub async fn get_changelist_with_file_revisions(
    changelist_id: i64,
) -> Result<(changelists::Model, Vec<file_revisions::Model>), DaoError> {
    let cl = changelists::Entity::find_by_id(changelist_id)
        .one(db()?)
        .await?;

    let cl = cl.ok_or_else(|| {
        DaoError::Db(DbErr::RecordNotFound(format!(
            "changelist not found for id: {changelist_id}"
        )))
    })?;

    let revisions = cl.find_related(file_revisions::Entity).all(db()?).await?;
    Ok((cl, revisions))
}

/// 按 depot path 查询该文件的最新 revision。
///
/// 比较规则：先比较 `generation`，大的更新；若 `generation` 相同，则比较 `revision`，大的更新。
pub async fn get_latest_revision_by_depot_path(
    path: &DepotPath,
) -> Result<file_revisions::Model, DaoError> {
    let key = ltree_key::depot_path_str_to_ltree_key(&path.to_string())?;

    let model = file_revisions::Entity::find()
        .filter(file_revisions::Column::Path.eq(key))
        .order_by_desc(file_revisions::Column::Generation)
        .order_by_desc(file_revisions::Column::Revision)
        .one(db()?)
        .await?;

    model.ok_or_else(|| {
        DaoError::Db(DbErr::RecordNotFound(format!(
            "latest revision not found for depot path: {path}"
        )))
    })
}

/// 批量按 depot path 查询每个文件的最新 revision。
///
/// 比较规则：先比较 `generation`，大的更新；若 `generation` 相同，则比较 `revision`，大的更新。
/// 返回结果顺序与入参 `paths` 一致；若某个 path 找不到任何 revision，则直接返回错误。
pub async fn get_latest_revisions_by_depot_paths(
    paths: &[DepotPath],
) -> Result<Vec<file_revisions::Model>, DaoError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut keys: Vec<String> = Vec::with_capacity(paths.len());
    for p in paths {
        keys.push(ltree_key::depot_path_str_to_ltree_key(&p.to_string())?);
    }

    let keys_for_query = keys.clone();

    // 一次性拉取所有 candidate，然后利用排序保证“每个 path 的第一条就是最新”。
    let models: Vec<file_revisions::Model> = file_revisions::Entity::find()
        .filter(file_revisions::Column::Path.is_in(keys_for_query))
        .order_by_asc(file_revisions::Column::Path)
        .order_by_desc(file_revisions::Column::Generation)
        .order_by_desc(file_revisions::Column::Revision)
        .all(db()?)
        .await?;

    let mut latest_by_key: HashMap<String, file_revisions::Model> = HashMap::new();
    for m in models {
        // 已按 (path asc, generation desc, revision desc) 排序：首次出现即最新
        latest_by_key.entry(m.path.clone()).or_insert(m);
    }

    let mut out = Vec::with_capacity(paths.len());
    for (p, key) in paths.iter().zip(keys.iter()) {
        let Some(m) = latest_by_key.remove(key) else {
            return Err(DaoError::Db(DbErr::RecordNotFound(format!(
                "latest revision not found for depot path: {p}"
            ))));
        };
        out.push(m);
    }

    Ok(out)
}

fn depot_dir_or_wildcard_to_ltree_prefix(path: &str) -> Result<String, DaoError> {
    let mut base = path.trim().to_string();
    if base.ends_with("...") {
        base.truncate(base.len() - 3);
    }
    if !base.ends_with('/') {
        base.push('/');
    }

    let dummy_path = format!("{base}__ltree_prefix__");
    let encoded = ltree_key::depot_path_str_to_ltree_key(&dummy_path)?;
    let (prefix, _) = encoded
        .rsplit_once('.')
        .ok_or_else(|| ltree_key::LtreeKeyError::InvalidLtreeKey(encoded.clone()))?;
    Ok(prefix.to_string())
}

fn ltree_depth(prefix: &str) -> i64 {
    if prefix.is_empty() {
        0
    } else {
        prefix.split('.').count() as i64
    }
}

/// 获取指定 depot path 的文件树（按 changelist_id 截止）。
///
/// - 文件路径：返回该文件最新 revision（若存在）。
/// - 目录路径：仅返回该目录“直接子级文件”的最新 revision。
/// - 通配路径（`//a/b/...`）：返回目录下所有后代文件的最新 revision。
pub async fn get_file_tree_revisions(
    depot: &DepotPath,
    changelist_id: i64,
) -> Result<Vec<file_revisions::Model>, DaoError> {
    let txn = db()?.begin().await?;

    let changelist_id_value = changelist_id;
    let models = if depot.is_file() {
        let key = ltree_key::depot_path_str_to_ltree_key(&depot.to_string())?;
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            SELECT DISTINCT ON (path)
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
              AND ($2::bigint <= 0 OR changelist_id <= $2)
            ORDER BY path, generation DESC, revision DESC, changelist_id DESC
            "#,
            [key.into(), changelist_id_value.into()].to_vec(),
        );
        file_revisions::Entity::find()
            .from_raw_sql(stmt)
            .all(&txn)
            .await?
    } else if depot.is_directory() {
        let prefix = depot_dir_or_wildcard_to_ltree_prefix(&depot.to_string())?;
        let depth = ltree_depth(&prefix) + 1;
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            SELECT DISTINCT ON (path)
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
            WHERE path <@ $1::ltree
              AND ($2::bigint <= 0 OR changelist_id <= $2)
              AND nlevel(path) = $3
            ORDER BY path, generation DESC, revision DESC, changelist_id DESC
            "#,
            [prefix.into(), changelist_id_value.into(), depth.into()].to_vec(),
        );
        file_revisions::Entity::find()
            .from_raw_sql(stmt)
            .all(&txn)
            .await?
    } else {
        let prefix = depot_dir_or_wildcard_to_ltree_prefix(&depot.to_string())?;
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            SELECT DISTINCT ON (path)
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
            WHERE path <@ $1::ltree
              AND ($2::bigint <= 0 OR changelist_id <= $2)
            ORDER BY path, generation DESC, revision DESC, changelist_id DESC
            "#,
            [prefix.into(), changelist_id_value.into()].to_vec(),
        );
        file_revisions::Entity::find()
            .from_raw_sql(stmt)
            .all(&txn)
            .await?
    };

    txn.commit().await?;
    Ok(models)
}