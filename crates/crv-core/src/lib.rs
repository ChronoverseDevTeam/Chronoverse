pub mod config;
pub mod server;
pub mod db;
pub mod auth;
pub mod api;
pub mod storage;
pub mod engine;

use crv_shared::error::Result;
use std::sync::Arc;

/// Application state shared across all Axum handlers.
pub struct AppState {
    pub config: config::Config,
    pub db_pool: sqlx::PgPool,
}

/// Initialize the application: load config, connect DB, build state.
pub async fn init() -> Result<Arc<AppState>> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    let config = config::Config::from_env()?;
    tracing::info!("Connecting to database at {}", config.database_url);

    let db_pool = db::pool::create_pool(&config.database_url).await?;
    db::pool::run_migrations(&db_pool).await?;

    tracing::info!("Database connected and migrated");

    Ok(Arc::new(AppState { config, db_pool }))
}
