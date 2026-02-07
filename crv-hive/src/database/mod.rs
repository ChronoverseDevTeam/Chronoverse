pub mod dao;
pub mod entities;
pub mod ltree_key;
pub mod migration;
pub mod service;

use anyhow::Result;
use once_cell::sync::OnceCell;
use urlencoding::encode;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::config::{entity::ConfigEntity, holder::get_or_init_config};

static DB_CONN: OnceCell<DatabaseConnection> = OnceCell::new();

fn postgres_connection_url(config: &ConfigEntity) -> String {
    let username = encode(&config.postgres_username);
    let password = encode(&config.postgres_password);

    format!(
        "postgresql://{}:{}@{}:{}/{}?client_encoding=UTF8&TimeZone=Asia/Shanghai",
        username,
        password,
        &config.postgres_hostname,
        &config.postgres_port,
        &config.postgres_database,
    )
}

pub async fn init() -> Result<()> {
    let config = get_or_init_config();
    let conn = Database::connect(&postgres_connection_url(config)).await?;

    // 使用 advisory lock 串行化 migration，避免多进程并发导致扩展/类型冲突
    let _ = conn
        .execute(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pg_advisory_lock(248031657)".to_string(),
        ))
        .await;

    // migrations (idempotent)
    let migrate_result = migration::Migrator::up(&conn, None).await;

    let _ = conn
        .execute(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pg_advisory_unlock(248031657)".to_string(),
        ))
        .await;

    migrate_result?;

    DB_CONN
        .set(conn)
        .map_err(|_| anyhow::anyhow!("Database already initialized"))?;

    Ok(())
}

pub async fn shutdown() -> Result<()> {
    let conn = DB_CONN
        .get()
        .ok_or(anyhow::anyhow!("Database not initialized"))?;
    conn.clone().close().await?;
    Ok(())
}

/// 获取数据库连接（全局单例）
pub fn get() -> &'static DatabaseConnection {
    DB_CONN.get().expect("Database not initialized")
}

/// 获取数据库连接（可选）
pub fn try_get() -> Option<&'static DatabaseConnection> {
    DB_CONN.get()
}
