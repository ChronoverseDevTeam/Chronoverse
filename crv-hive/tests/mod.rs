use std::sync::Arc;

use crv_core::cas::CasStore;
use crv_hive::crv2::{
    config::{DatabaseConfig, IrohConfig},
    postgres::PostgreExecutor,
    ChronoverseApp,
    iroh::{
        iroh_client::{IrohClient, ALPN},
        service::IrohService,
    },
};
use crv_relay::relay::RelayServer;
use iroh_blobs::ticket::BlobTicket;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Start an embedded relay on an ephemeral port and a bound server iroh
/// endpoint.  Returns `(relay, relay_url, server_client)`.
async fn start_test_server() -> (RelayServer, String, IrohClient) {
    let relay = RelayServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("relay start");

    let relay_port = relay.http_addr().expect("relay has http addr").port();
    let relay_url = format!("http://127.0.0.1:{relay_port}");

    let server = IrohClient::start(&IrohConfig {
        relay_url: relay_url.clone(),
        ..IrohConfig::default()
    })
    .await
    .expect("server iroh start");

    (relay, relay_url, server)
}

fn test_database_config() -> DatabaseConfig {
    let defaults = DatabaseConfig::default();
    DatabaseConfig {
        url: defaults.test_url.clone(),
        test_url: defaults.test_url,
        max_connections: 5,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

/// Connect an iroh client to the [`IrohService`] and call `register_user`.
///
/// Test flow:
/// 1. Start an embedded relay on a random port.
/// 2. Start a server-side iroh endpoint and wrap it in [`IrohService`].
/// 3. Start a client-side iroh endpoint and establish a connection.
/// 4. Open a bi-directional stream, send a `register_user` JSON request.
/// 5. Read the JSON response and assert the returned username.
#[tokio::test]
async fn test_register_user_over_iroh() {
    let (relay, relay_url, server) = start_test_server().await;
    let postgres = Arc::new(
        PostgreExecutor::connect_and_init(&test_database_config())
            .await
            .expect("postgres executor start"),
    );
    let cas_store = CasStore::memory();
    // Save the server address before moving `server` into the service.
    let server_addr = server.addr();

    // Wrap the server endpoint in the service and run it in a background task.
    let app = Arc::new(ChronoverseApp::new(Arc::clone(&postgres), cas_store, server_addr.clone()));
    let service = IrohService::new(server, app);
    let service_task = tokio::spawn(async move { service.serve().await });

    // Yield once so the service task has a chance to call accept().
    tokio::task::yield_now().await;

    // Create a client endpoint connected to the same relay.
    let client = IrohClient::start(&IrohConfig {
        relay_url: relay_url.clone(),
        ..IrohConfig::default()
    })
    .await
    .expect("client iroh start");

    // Establish a QUIC connection to the server (routed through the relay).
    let conn = client.connect(server_addr, ALPN).await.expect("connect to server");

    // Open a bi-directional stream.
    let (mut send, recv) = conn.open_bi().await.expect("open_bi");
    let username = format!("alice-{}", Uuid::new_v4().simple());

    // Send a register_user request as a newline-terminated JSON line.
    let req = format!(
        r#"{{"method":"register_user","username":"{}","password":"secret"}}"#,
        username
    );
    send.write_all(req.as_bytes()).await.expect("write request");
    send.write_all(b"\n").await.expect("write request terminator");
    let _ = send.finish();

    // Read the newline-terminated JSON response.
    let mut reader = BufReader::new(recv);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("read response");

    // Verify the response.
    let resp: serde_json::Value =
        serde_json::from_str(line.trim()).expect("parse JSON response");

    assert_eq!(resp["ok"], true, "expected ok=true, got: {resp}");
    assert_eq!(
        resp["data"]["username"],
        username,
        "expected generated username, got: {resp}"
    );

    // Graceful cleanup.
    client.shutdown().await;
    service_task.abort();
    postgres.close().await.expect("postgres shutdown");
    relay.shutdown().await.expect("relay shutdown");
}

#[tokio::test]
async fn test_download_blob_via_blob_ticket_over_iroh_blobs() {
    let (relay, relay_url, server) = start_test_server().await;
    let postgres = Arc::new(
        PostgreExecutor::connect_and_init(&test_database_config())
            .await
            .expect("postgres executor start"),
    );
    let cas_store = CasStore::memory();
    let pin = cas_store.put_bytes("blob payload").await.expect("blob should store");
    let server_addr = server.addr();
    let app = Arc::new(ChronoverseApp::new(Arc::clone(&postgres), cas_store, server_addr.clone()));
    let service = IrohService::new(server, app);
    let service_task = tokio::spawn(async move { service.serve().await });

    tokio::task::yield_now().await;

    let client = IrohClient::start(&IrohConfig {
        relay_url: relay_url.clone(),
        ..IrohConfig::default()
    })
    .await
    .expect("client iroh start");

    let conn = client.connect(server_addr, ALPN).await.expect("connect to server");
    let (mut send, recv) = conn.open_bi().await.expect("open_bi");
    let req = format!(
        r#"{{"method":"get_blob_ticket","hash":"{}"}}"#,
        pin.hash()
    );
    send.write_all(req.as_bytes()).await.expect("write request");
    send.write_all(b"\n").await.expect("write request terminator");
    let _ = send.finish();

    let mut reader = BufReader::new(recv);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("read response");
    let resp: serde_json::Value = serde_json::from_str(line.trim()).expect("parse JSON response");
    assert_eq!(resp["ok"], true, "expected ok=true, got: {resp}");
    let ticket: BlobTicket = resp["data"]["ticket"]
        .as_str()
        .expect("ticket string")
        .parse()
        .expect("parse blob ticket");

    let conn = client
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .expect("connect blob protocol");
    let stream = iroh_blobs::get::request::get_blob(conn, ticket.hash());
    let (bytes, _stats) = stream.bytes_and_stats().await.expect("download blob bytes");
    assert_eq!(bytes.as_ref(), b"blob payload");

    client.shutdown().await;
    service_task.abort();
    postgres.close().await.expect("postgres shutdown");
    relay.shutdown().await.expect("relay shutdown");
}
