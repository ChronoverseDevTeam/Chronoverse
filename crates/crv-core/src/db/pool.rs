use crv_shared::error::{CrvError, Result};
use sqlx::postgres::{PgPoolOptions, PgConnectOptions};

/// Create a PostgreSQL connection pool.
pub async fn create_pool(database_url: &str) -> Result<sqlx::PgPool> {
    let opts: PgConnectOptions = database_url
        .parse()
        .map_err(|e| CrvError::Database(format!("invalid database URL: {e}")))?;

    // Disable SSL for local/dev environments
    let opts = opts.ssl_mode(sqlx::postgres::PgSslMode::Disable);

    PgPoolOptions::new()
        .max_connections(20)
        .connect_with(opts)
        .await
        .map_err(|e| CrvError::Database(format!("failed to connect to database: {e}")))
}

/// Run pending SQLx migrations from `crates/crv-core/migrations/`.
pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| CrvError::Database(format!("migration failed: {e}")))?;
    Ok(())
}
