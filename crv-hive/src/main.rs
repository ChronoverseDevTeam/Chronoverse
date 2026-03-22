use std::sync::Arc;

use crv_hive::{crv2::{
    iroh::{
        captive_portal::CaptivePortalServer,
        iroh_client::{IrohClient, IrohClientConfig},
        relay::RelayServer,
        service::IrohService,
    },
    ChronoverseApp,
}, logging};

const RELAY_ADDR: &str = "0.0.0.0:3340";
const RELAY_URL: &str = "http://127.0.0.1:3340";
// iroh's captive-portal probe always targets port 80 on the relay host.
// We bind a lightweight responder there so the probe succeeds immediately.
const CAPTIVE_PORTAL_ADDR: &str = "0.0.0.0:80";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    logging::init_logging();
    let logger = logging::HiveLog::new("main");
    // ── 1. Start the embedded iroh relay ────────────────────────────────────
    let relay = RelayServer::start(RELAY_ADDR.parse()?).await?;
    logger.info(&format!("iroh relay  : {:?}", relay.http_addr()));

    // ── 2. Start the captive-portal responder (port 80) ──────────────────────
    let captive = CaptivePortalServer::start(CAPTIVE_PORTAL_ADDR.parse()?).await?;
    logger.info(&format!("captive-portal responder: {}", captive.addr()));

    // ── 3. Start the crv-hive iroh endpoint ─────────────────────────────────
    let iroh = IrohClient::start(IrohClientConfig {
        relay_url: Some(RELAY_URL.parse()?),
        secret_key: None,
    })
    .await?;
    logger.info(&format!("hive node id: {}", iroh.id()));
    logger.info(&format!("hive addr   : {:?}", iroh.addr()));

    // Publish the ticket so GET /crv/node-ticket is served immediately.
    let ticket = iroh.ticket().to_string();
    logger.info(&format!("node ticket : {ticket}"));
    captive.set_ticket(ticket);

    // ── 4. Wrap iroh client in the service (handles register_user etc.) ──────
    let app = Arc::new(ChronoverseApp::new());
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

    Ok(())
}
