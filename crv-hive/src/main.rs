use std::sync::Arc;

use crv_core::cas::CasStore;
use crv_hive::{crv2::{
    config::{self, ConfigSource},
    postgres::PostgreExecutor,
    iroh::{
        iroh_client::IrohClient,
        rpc_server::IrohRpcServer,
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
    let app = Arc::new(ChronoverseApp::new(Arc::clone(&postgres), cas_store.clone(), iroh_addr.clone()));
    let rpc_server = IrohRpcServer::new(iroh, app);

    // ── 3. Publish hive ticket to the captive-portal so edge clients can discover us
    {
        let ticket = iroh_tickets::endpoint::EndpointTicket::from(iroh_addr).to_string();
        let pkarr_url = &config.iroh.pkarr_url;
        match publish_hive_ticket(pkarr_url, &ticket).await {
            Ok(()) => logger.info("hive ticket : published to captive-portal"),
            Err(e) => logger.info(&format!("hive ticket : publish failed ({e})")),
        }
    }

    // ── 4. Run until Ctrl-C ──────────────────────────────────────────────────
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            logger.info("Shutting down...");
        }
        _ = rpc_server.serve() => {}
    }
    postgres.close().await?;
	cas_store.shutdown().await?;

    Ok(())
}

/// PUT the hive ticket to `{pkarr_url}/hive_ticket` using a minimal
/// HTTP/1.1 request over a raw TCP socket (no extra dependencies).
async fn publish_hive_ticket(pkarr_url: &str, ticket: &str) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    // Parse host:port from the URL (e.g. "http://127.0.0.1:80").
    let url: url::Url = pkarr_url.parse()?;
    let host = url.host_str().unwrap_or("127.0.0.1");
    let port = url.port().unwrap_or(80);
    let addr = format!("{host}:{port}");

    let stream = TcpStream::connect(&addr).await?;
    let (reader, mut writer) = stream.into_split();

    let req = format!(
        "PUT /hive_ticket HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        ticket.len(),
        ticket
    );
    writer.write_all(req.as_bytes()).await?;

    // Read status line to confirm success.
    let mut reader = BufReader::new(reader);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).await?;

    if !status_line.contains("200") {
        anyhow::bail!("server responded: {}", status_line.trim());
    }

    Ok(())
}
