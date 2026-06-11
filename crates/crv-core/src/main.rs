use crv_shared::error::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "crv_core=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting crv-core v{}", env!("CARGO_PKG_VERSION"));

    let state = crv_core::init().await?;

    let router = crv_core::server::build_router(state.clone());
    let listen_addr = state.config.listen_addr();

    tracing::info!("Listening on {}", listen_addr);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .map_err(|e| crv_shared::error::CrvError::Network(format!("bind failed: {e}")))?;

    axum::serve(listener, router)
        .await
        .map_err(|e| crv_shared::error::CrvError::Network(format!("server error: {e}")))?;

    Ok(())
}
