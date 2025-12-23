pub mod dao;
pub mod entities;
pub mod migration;

use anyhow::Result;
use once_cell::sync::OnceCell;
use urlencoding::encode;
use sea_orm::{Database, DatabaseConnection};
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

    // migrations (idempotent)
    migration::Migrator::up(&conn, None).await?;

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
