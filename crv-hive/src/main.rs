use std::sync::Arc;

use crv_core::cas::CasStore;
use crv_hive::{crv2::{
    config::{self, ConfigSource},
    postgres::PostgreExecutor,
    iroh::{
        iroh_client::IrohClient,
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

    // ── 1. Start the crv-hive iroh endpoint ─────────────────────────────────
    let iroh = IrohClient::start(&config.iroh).await?;
    let iroh_addr = iroh.addr();
    logger.info(&format!("hive node id: {}", iroh.id()));
    logger.info(&format!("hive addr   : {:?}", iroh_addr));

    // ── 2. Wrap iroh client in the service (handles register_user etc.) ──────
    let app = Arc::new(ChronoverseApp::new(Arc::clone(&postgres), cas_store.clone(), iroh_addr));
    let service = IrohService::new(iroh, app);

    // ── 3. Run until Ctrl-C ──────────────────────────────────────────────────
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            logger.info("Shutting down...");
        }
        _ = service.serve() => {}
    }
    postgres.close().await?;
	cas_store.shutdown().await?;

    Ok(())
}
