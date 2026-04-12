use crv_relay::{
    captive_portal::CaptivePortalServer,
    config::{self, ConfigSource},
    relay::RelayServer,
};

fn init_logging(default_filter: &str) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::new(default_filter);

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .try_init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loaded_config = config::init_from_args()?;
    init_logging(&loaded_config.config.logging.rust_log);
    let config = config::current();

    match &loaded_config.source {
        ConfigSource::Defaults => tracing::info!("config source : internal defaults"),
        ConfigSource::File(path) => {
            tracing::info!("config source : {}", path.display());
        }
    }

    // ── 1. Start the embedded iroh relay ────────────────────────────────────
    let relay = RelayServer::start(config.relay.relay_bind_addr.parse()?).await?;
    tracing::info!("iroh relay  : {:?}", relay.http_addr());

    // ── 2. Start the captive-portal / Pkarr relay responder ────────────────
    let captive = CaptivePortalServer::start(config.relay.captive_portal_addr.parse()?).await?;
    tracing::info!("captive-portal / pkarr relay: {}", captive.addr());
    tracing::info!("pkarr relay url             : {}", captive.pkarr_url());

    // ── 3. Run until Ctrl-C ─────────────────────────────────────────────────
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    captive.shutdown();
    relay.shutdown().await?;

    Ok(())
}
