use std::sync::Arc;

use crv_core::cas::CasStore;
use crv_hive::{crv2::{
    config::{self, ConfigSource},
    postgres::PostgreExecutor,
    iroh::{
        captive_portal::CaptivePortalServer,
        iroh_client::{IrohClient, IrohClientConfig},
        relay::RelayServer,
        service::IrohService,
    },
    ChronoverseApp,
}, logging};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loaded_config = config::init_from_args()?;
    logging::init_logging_with_filter(&loaded_config.config.logging.rust_log);
    let logger = logging::HiveLog::new("main");
    let config = config::current();

    match &loaded_config.source {
        ConfigSource::Defaults => logger.info("config source : internal defaults"),
        ConfigSource::File(path) => {
            logger.info(&format!("config source : {}", path.display()));
        }
    }

    let postgres = Arc::new(PostgreExecutor::connect_and_init(&config.database).await?);
    logger.info("postgres pool : ready");
    let cas_store = CasStore::persistent(config.storage.repository_path.join("cas")).await?;
    logger.info("cas store : ready");

    // ── 1. Start the embedded iroh relay ────────────────────────────────────
    let relay = RelayServer::start(config.iroh.relay_bind_addr.parse()?).await?;
    logger.info(&format!("iroh relay  : {:?}", relay.http_addr()));

    // ── 2. Start the captive-portal responder (port 80) ──────────────────────
    let captive = CaptivePortalServer::start(config.iroh.captive_portal_addr.parse()?).await?;
    logger.info(&format!("captive-portal responder: {}", captive.addr()));

    // ── 3. Start the crv-hive iroh endpoint ─────────────────────────────────
    let iroh = IrohClient::start(IrohClientConfig {
        relay_url: Some(config.iroh.relay_url.parse()?),
        secret_key: None,
    })
    .await?;
    let iroh_addr = iroh.addr();
    logger.info(&format!("hive node id: {}", iroh.id()));
    logger.info(&format!("hive addr   : {:?}", iroh_addr));

    // Publish the ticket so GET /crv/node-ticket is served immediately.
    let ticket = iroh.ticket().to_string();
    logger.info(&format!("node ticket : {ticket}"));
    captive.set_ticket(ticket);

    // ── 4. Wrap iroh client in the service (handles register_user etc.) ──────
    let app = Arc::new(ChronoverseApp::new(Arc::clone(&postgres), cas_store.clone(), iroh_addr));
    let service = IrohService::new(iroh, app);

    // ── 5. Run until Ctrl-C ──────────────────────────────────────────────────
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            logger.info("Shutting down...");
        }
        _ = service.serve() => {}
    }
    captive.shutdown();
    relay.shutdown().await?;
    postgres.close().await?;
	cas_store.shutdown().await?;

    Ok(())
}
